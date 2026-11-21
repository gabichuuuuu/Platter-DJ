//! The U-Net that estimates the spectrum of one stem from the spectrum of the mixture, as the
//! 4-stem Spleeter model is built: six 5 by 5 convolutions of stride two with batch
//! normalisation and ELU, six transposed convolutions back with the outputs of the encoder
//! joined in, and a 4 by 4 dilated convolution that gives a sigmoid mask, multiplied by the
//! input. Every map is channels last, as the weights are.
//!
//! Choices: `docs/decisions/0038-stem-separation.md`.

/// A map of `height` by `width` positions of `channels` values, channels last.
#[derive(Clone, Debug, PartialEq)]
pub struct Map {
    pub height: usize,
    pub width: usize,
    pub channels: usize,
    pub data: Vec<f32>,
}

impl Map {
    pub fn zeros(height: usize, width: usize, channels: usize) -> Self {
        Self {
            height,
            width,
            channels,
            data: vec![0.0; height * width * channels],
        }
    }

    /// Joins the channels of `first` and of `second`, in that order, at each position.
    fn join(first: &Self, second: &Self) -> Option<Self> {
        if (first.height, first.width) != (second.height, second.width) {
            return None;
        }
        let channels = first.channels + second.channels;
        let mut data = Vec::with_capacity(first.height * first.width * channels);
        for (a, b) in first
            .data
            .chunks_exact(first.channels)
            .zip(second.data.chunks_exact(second.channels))
        {
            data.extend_from_slice(a);
            data.extend_from_slice(b);
        }
        Some(Self {
            height: first.height,
            width: first.width,
            channels,
            data,
        })
    }
}

/// A convolution with the "same" padding of TensorFlow: the output has the size of the input
/// divided by the stride, rounded up, and the padding goes one more at the end when it is odd.
#[derive(Clone, Debug, PartialEq)]
pub struct Conv {
    pub size: usize,
    pub stride: usize,
    pub dilation: usize,
    pub inputs: usize,
    pub outputs: usize,
    /// `[row][column][input][output]`.
    pub kernel: Vec<f32>,
    pub bias: Vec<f32>,
    /// A transposed convolution: the output is the input times the stride.
    pub transposed: bool,
}

/// Batch normalisation at inference, folded into a scale and a shift per channel.
#[derive(Clone, Debug, PartialEq)]
pub struct Norm {
    pub scale: Vec<f32>,
    pub shift: Vec<f32>,
}

impl Norm {
    /// From the four values Keras keeps, with its default epsilon of 0.001.
    pub fn from_batch(gamma: &[f32], beta: &[f32], mean: &[f32], variance: &[f32]) -> Self {
        let scale: Vec<f32> = gamma
            .iter()
            .zip(variance)
            .map(|(gamma, variance)| gamma / (variance + 0.001).sqrt())
            .collect();
        let shift = beta
            .iter()
            .zip(mean)
            .zip(&scale)
            .map(|((beta, mean), scale)| beta - mean * scale)
            .collect();
        Self { scale, shift }
    }

    fn apply(&self, map: &mut Map) {
        for position in map.data.chunks_exact_mut(map.channels) {
            for ((value, scale), shift) in position.iter_mut().zip(&self.scale).zip(&self.shift) {
                *value = *value * scale + shift;
            }
        }
    }
}

fn elu(map: &mut Map) {
    for value in &mut map.data {
        if *value < 0.0 {
            *value = value.exp() - 1.0;
        }
    }
}

/// The padding before the first position along one axis, as TensorFlow places it.
fn padding_before(input: usize, output: usize, stride: usize, span: usize) -> usize {
    ((output.saturating_sub(1)) * stride + span).saturating_sub(input) / 2
}

impl Conv {
    fn span(&self) -> usize {
        (self.size - 1) * self.dilation + 1
    }

    fn fits(&self, input: &Map) -> bool {
        input.channels == self.inputs
            && self.kernel.len() == self.size * self.size * self.inputs * self.outputs
            && self.bias.len() == self.outputs
            && self.stride > 0
            && self.dilation > 0
    }

    /// The convolution of `input`; `None` when the input does not have the channels the
    /// kernel takes.
    pub fn apply(&self, input: &Map, threads: usize) -> Option<Map> {
        if !self.fits(input) {
            return None;
        }
        let (height, width) = if self.transposed {
            (input.height * self.stride, input.width * self.stride)
        } else {
            (
                input.height.div_ceil(self.stride),
                input.width.div_ceil(self.stride),
            )
        };
        let mut output = Map::zeros(height, width, self.outputs);
        let row_len = width * self.outputs;
        let rows_per_part = height.div_ceil(threads.max(1)).max(1);
        std::thread::scope(|scope| {
            for (part, rows) in output.data.chunks_mut(rows_per_part * row_len).enumerate() {
                let first = part * rows_per_part;
                scope.spawn(move || {
                    for (offset, row) in rows.chunks_mut(row_len).enumerate() {
                        if self.transposed {
                            self.transposed_row(input, first + offset, row);
                        } else {
                            self.row(input, first + offset, row);
                        }
                    }
                });
            }
        });
        Some(output)
    }

    /// One row of a forward convolution.
    fn row(&self, input: &Map, y: usize, row: &mut [f32]) {
        let out_h = input.height.div_ceil(self.stride);
        let out_w = input.width.div_ceil(self.stride);
        let top = padding_before(input.height, out_h, self.stride, self.span());
        let left = padding_before(input.width, out_w, self.stride, self.span());
        let block = self.inputs * self.outputs;
        for (x, out) in row.chunks_exact_mut(self.outputs).enumerate() {
            out.copy_from_slice(&self.bias);
            for ky in 0..self.size {
                let Some(iy) = (y * self.stride + ky * self.dilation)
                    .checked_sub(top)
                    .filter(|iy| *iy < input.height)
                else {
                    continue;
                };
                for kx in 0..self.size {
                    let Some(ix) = (x * self.stride + kx * self.dilation)
                        .checked_sub(left)
                        .filter(|ix| *ix < input.width)
                    else {
                        continue;
                    };
                    let at = (iy * input.width + ix) * self.inputs;
                    let pixel = &input.data[at..at + self.inputs];
                    let weights = &self.kernel[(ky * self.size + kx) * block..][..block];
                    accumulate(out, pixel, weights);
                }
            }
        }
    }

    /// One row of a transposed convolution, gathered from the input positions that reach it.
    fn transposed_row(&self, input: &Map, y: usize, row: &mut [f32]) {
        let height = input.height * self.stride;
        let width = input.width * self.stride;
        // The padding of the forward convolution this one is the transpose of.
        let top = padding_before(height, input.height, self.stride, self.span());
        let left = padding_before(width, input.width, self.stride, self.span());
        let block = self.inputs * self.outputs;
        for (x, out) in row.chunks_exact_mut(self.outputs).enumerate() {
            out.copy_from_slice(&self.bias);
            for ky in 0..self.size {
                let Some(iy) = (y + top)
                    .checked_sub(ky * self.dilation)
                    .filter(|reach| reach % self.stride == 0)
                    .map(|reach| reach / self.stride)
                    .filter(|iy| *iy < input.height)
                else {
                    continue;
                };
                for kx in 0..self.size {
                    let Some(ix) = (x + left)
                        .checked_sub(kx * self.dilation)
                        .filter(|reach| reach % self.stride == 0)
                        .map(|reach| reach / self.stride)
                        .filter(|ix| *ix < input.width)
                    else {
                        continue;
                    };
                    let at = (iy * input.width + ix) * self.inputs;
                    let pixel = &input.data[at..at + self.inputs];
                    let weights = &self.kernel[(ky * self.size + kx) * block..][..block];
                    accumulate(out, pixel, weights);
                }
            }
        }
    }
}

/// `out += pixel · weights`, with `weights` as `[input][output]`.
fn accumulate(out: &mut [f32], pixel: &[f32], weights: &[f32]) {
    for (value, column) in pixel.iter().zip(weights.chunks_exact(out.len())) {
        if *value == 0.0 {
            continue;
        }
        for (sum, weight) in out.iter_mut().zip(column) {
            *sum += value * weight;
        }
    }
}

/// The filters of the encoder, deepest last.
pub const FILTERS: [usize; 6] = [16, 32, 64, 128, 256, 512];

/// The network of one stem.
#[derive(Clone, Debug, PartialEq)]
pub struct UNet {
    pub encoder: Vec<(Conv, Norm)>,
    pub decoder: Vec<(Conv, Norm)>,
    pub last: Conv,
}

impl UNet {
    /// The spectrum of the stem: the mask of the network times `input`. `None` when the
    /// weights do not fit the input.
    pub fn apply(&self, input: &Map, threads: usize) -> Option<Map> {
        if self.encoder.len() != 6 || self.decoder.len() != 6 {
            return None;
        }
        // The raw outputs of the convolutions are what the decoder joins in.
        let mut raw = Vec::with_capacity(6);
        let mut next = input.clone();
        for (conv, norm) in &self.encoder {
            let out = conv.apply(&next, threads)?;
            let mut activated = out.clone();
            norm.apply(&mut activated);
            elu(&mut activated);
            raw.push(out);
            next = activated;
        }
        let mut up = raw.pop()?;
        for (index, (conv, norm)) in self.decoder.iter().enumerate() {
            let mut out = conv.apply(&up, threads)?;
            elu(&mut out);
            norm.apply(&mut out);
            up = match raw.pop() {
                Some(skip) if index < 5 => Map::join(&skip, &out)?,
                _ => out,
            };
        }
        let mut mask = self.last.apply(&up, threads)?;
        if mask.data.len() != input.data.len() {
            return None;
        }
        for (value, original) in mask.data.iter_mut().zip(&input.data) {
            *value = original / (1.0 + (-*value).exp());
        }
        Some(mask)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conv(size: usize, stride: usize, inputs: usize, outputs: usize, transposed: bool) -> Conv {
        Conv {
            size,
            stride,
            dilation: 1,
            inputs,
            outputs,
            kernel: vec![0.0; size * size * inputs * outputs],
            bias: vec![0.0; outputs],
            transposed,
        }
    }

    #[test]
    fn a_strided_convolution_pads_as_tensorflow_does() {
        // A 4 by 4 map of ones, a 5 by 5 kernel of ones, stride two: padding one before and two
        // after, so the first output sees 4 by 4 values and the last 3 by 3.
        let input = Map {
            height: 4,
            width: 4,
            channels: 1,
            data: vec![1.0; 16],
        };
        let mut layer = conv(5, 2, 1, 1, false);
        layer.kernel.fill(1.0);
        let out = layer.apply(&input, 2).unwrap();
        assert_eq!((out.height, out.width), (2, 2));
        assert_eq!(out.data, vec![16.0, 12.0, 12.0, 9.0]);
    }

    #[test]
    fn a_transposed_convolution_is_the_transpose_of_the_forward_one() {
        // <forward(x), y> == <x, transposed(y)> for the same kernel.
        let mut forward = conv(5, 2, 2, 3, false);
        for (index, weight) in forward.kernel.iter_mut().enumerate() {
            *weight = ((index * 7919) % 13) as f32 / 13.0 - 0.5;
        }
        // The transposed layer maps 3 channels back to 2: its kernel is the same numbers read
        // as [row][column][input = 3][output = 2].
        let mut backward = conv(5, 2, 3, 2, true);
        for ky in 0..5 {
            for kx in 0..5 {
                for i in 0..2 {
                    for o in 0..3 {
                        backward.kernel[((ky * 5 + kx) * 3 + o) * 2 + i] =
                            forward.kernel[((ky * 5 + kx) * 2 + i) * 3 + o];
                    }
                }
            }
        }
        let x = Map {
            height: 6,
            width: 8,
            channels: 2,
            data: (0..96).map(|v| ((v * 31) % 17) as f32 / 17.0).collect(),
        };
        let y = Map {
            height: 3,
            width: 4,
            channels: 3,
            data: (0..36).map(|v| ((v * 11) % 7) as f32 / 7.0).collect(),
        };
        let fx = forward.apply(&x, 3).unwrap();
        let ty = backward.apply(&y, 1).unwrap();
        let left: f32 = fx.data.iter().zip(&y.data).map(|(a, b)| a * b).sum();
        let right: f32 = x.data.iter().zip(&ty.data).map(|(a, b)| a * b).sum();
        assert!((left - right).abs() < 1e-3, "{left} {right}");
    }

    #[test]
    fn the_folded_normalisation_matches_its_formula() {
        let norm = Norm::from_batch(&[2.0], &[0.5], &[1.0], &[3.999]);
        let mut map = Map {
            height: 1,
            width: 1,
            channels: 1,
            data: vec![3.0],
        };
        norm.apply(&mut map);
        assert!((map.data[0] - (2.0 * (3.0 - 1.0) / 2.0 + 0.5)).abs() < 1e-5);
    }

    #[test]
    fn a_unet_of_zero_weights_halves_its_input() {
        // Every layer gives zero, the mask is the sigmoid of zero: one half of the input.
        let mut encoder = Vec::new();
        let mut inputs = 2;
        for filters in FILTERS {
            encoder.push((
                conv(5, 2, inputs, filters, false),
                Norm::from_batch(
                    &vec![1.0; filters],
                    &vec![0.0; filters],
                    &vec![0.0; filters],
                    &vec![1.0; filters],
                ),
            ));
            inputs = filters;
        }
        let mut decoder = Vec::new();
        let outs = [256, 128, 64, 32, 16, 1];
        let ins = [512, 512, 256, 128, 64, 32];
        for (out, inp) in outs.into_iter().zip(ins) {
            decoder.push((
                conv(5, 2, inp, out, true),
                Norm::from_batch(
                    &vec![1.0; out],
                    &vec![0.0; out],
                    &vec![0.0; out],
                    &vec![1.0; out],
                ),
            ));
        }
        let mut last = conv(4, 1, 1, 2, false);
        last.dilation = 2;
        let net = UNet {
            encoder,
            decoder,
            last,
        };
        let input = Map {
            height: 64,
            width: 64,
            channels: 2,
            data: (0..8192).map(|v| (v % 5) as f32).collect(),
        };
        let out = net.apply(&input, 4).unwrap();
        for (a, b) in out.data.iter().zip(&input.data) {
            assert!((a - b / 2.0).abs() < 1e-5);
        }
    }
}
