//! The weights of a separation model in a file of Platter: named tensors of 32-bit floats.
//!
//! ```text
//! "PLTRWGT1"                      8 bytes
//! count                           u32
//! count times:
//!   name length, name             u16, UTF-8
//!   rank, dimensions              u8, u32 each
//!   values                        f32 each, as many as the dimensions multiply to
//! ```
//!
//! Every number is little-endian. A model trained later for Platter is written in the same
//! file, so it takes the place of the one installed today: `docs/decisions/0038-stem-separation.md`.

use std::collections::BTreeMap;

use crate::net::{Conv, FILTERS, Norm, UNet};

const MAGIC: &[u8; 8] = b"PLTRWGT1";
/// More tensors than a file of a model of four stems has.
pub const MAX_TENSORS: usize = 4096;
/// The most values of one tensor, and of a whole file: a 512 MiB file of floats.
pub const MAX_VALUES: usize = 128 * 1024 * 1024;
const MAX_RANK: usize = 8;

/// A tensor: its dimensions and its values, row-major.
#[derive(Clone, Debug, PartialEq)]
pub struct Tensor {
    pub dims: Vec<usize>,
    pub values: Vec<f32>,
}

/// The tensors of a model by name.
pub type Tensors = BTreeMap<String, Tensor>;

#[derive(Debug, PartialEq, Eq)]
pub enum WeightsError {
    /// Not a file of weights of Platter, or a damaged one.
    Format,
    /// A tensor the model needs is missing, or has another shape.
    Missing(String),
}

impl std::fmt::Display for WeightsError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Format => formatter.write_str("the file is not a model of Platter"),
            Self::Missing(name) => write!(formatter, "the model has no fitting tensor {name}"),
        }
    }
}

impl std::error::Error for WeightsError {}

pub fn write(tensors: &Tensors) -> Vec<u8> {
    let mut out = MAGIC.to_vec();
    out.extend_from_slice(&(tensors.len() as u32).to_le_bytes());
    for (name, tensor) in tensors {
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.push(tensor.dims.len() as u8);
        for dim in &tensor.dims {
            out.extend_from_slice(&(*dim as u32).to_le_bytes());
        }
        for value in &tensor.values {
            out.extend_from_slice(&value.to_le_bytes());
        }
    }
    out
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Cursor<'_> {
    fn take(&mut self, count: usize) -> Result<&[u8], WeightsError> {
        let end = self.at.checked_add(count).ok_or(WeightsError::Format)?;
        let slice = self.bytes.get(self.at..end).ok_or(WeightsError::Format)?;
        self.at = end;
        Ok(slice)
    }

    fn u32(&mut self) -> Result<u32, WeightsError> {
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }
}

/// Reads a file of weights; every count is bounded before anything is allocated for it.
pub fn read(bytes: &[u8]) -> Result<Tensors, WeightsError> {
    let mut cursor = Cursor { bytes, at: 0 };
    if cursor.take(8)? != MAGIC {
        return Err(WeightsError::Format);
    }
    let count = cursor.u32()? as usize;
    if count > MAX_TENSORS {
        return Err(WeightsError::Format);
    }
    let mut tensors = Tensors::new();
    let mut total = 0usize;
    for _ in 0..count {
        let length = cursor.take(2)?;
        let length = u16::from_le_bytes([length[0], length[1]]) as usize;
        let name = std::str::from_utf8(cursor.take(length)?)
            .map_err(|_| WeightsError::Format)?
            .to_owned();
        let rank = cursor.take(1)?[0] as usize;
        if rank > MAX_RANK {
            return Err(WeightsError::Format);
        }
        let mut dims = Vec::with_capacity(rank);
        let mut size = 1usize;
        for _ in 0..rank {
            let dim = cursor.u32()? as usize;
            size = size.checked_mul(dim).ok_or(WeightsError::Format)?;
            dims.push(dim);
        }
        total = total.checked_add(size).ok_or(WeightsError::Format)?;
        if size > MAX_VALUES || total > MAX_VALUES {
            return Err(WeightsError::Format);
        }
        let raw = cursor.take(size.checked_mul(4).ok_or(WeightsError::Format)?)?;
        let values = raw
            .as_chunks::<4>()
            .0
            .iter()
            .map(|bytes| f32::from_le_bytes(*bytes))
            .collect();
        tensors.insert(name, Tensor { dims, values });
    }
    if cursor.at != bytes.len() {
        return Err(WeightsError::Format);
    }
    Ok(tensors)
}

fn tensor<'a>(tensors: &'a Tensors, name: &str, dims: &[usize]) -> Result<&'a [f32], WeightsError> {
    tensors
        .get(name)
        .filter(|tensor| tensor.dims == dims && tensor.values.iter().all(|v| v.is_finite()))
        .map(|tensor| tensor.values.as_slice())
        .ok_or_else(|| WeightsError::Missing(name.to_owned()))
}

fn conv(
    tensors: &Tensors,
    name: &str,
    size: usize,
    (inputs, outputs): (usize, usize),
    transposed: bool,
) -> Result<Conv, WeightsError> {
    let kernel = tensor(
        tensors,
        &format!("{name}/kernel"),
        &[size, size, inputs, outputs],
    )?;
    let bias = tensor(tensors, &format!("{name}/bias"), &[outputs])?;
    Ok(Conv {
        size,
        stride: if transposed || size == 5 { 2 } else { 1 },
        dilation: if size == 4 { 2 } else { 1 },
        inputs,
        outputs,
        kernel: kernel.to_vec(),
        bias: bias.to_vec(),
        transposed,
    })
}

fn norm(tensors: &Tensors, name: &str, channels: usize) -> Result<Norm, WeightsError> {
    let part = |what: &str| tensor(tensors, &format!("{name}/{what}"), &[channels]);
    Ok(Norm::from_batch(
        part("gamma")?,
        part("beta")?,
        part("mean")?,
        part("variance")?,
    ))
}

/// The inputs and outputs of the six transposed convolutions of the decoder.
pub const DECODER: [(usize, usize); 6] = [
    (512, 256),
    (512, 128),
    (256, 64),
    (128, 32),
    (64, 16),
    (32, 1),
];

/// The network of `stem` from tensors named `<stem>/encoder<n>/...`, `<stem>/decoder<n>/...`
/// and `<stem>/last/...`. The kernels of the transposed convolutions are stored as
/// `[row][column][input][output]`, as the network reads them.
pub fn unet(tensors: &Tensors, stem: &str) -> Result<UNet, WeightsError> {
    let mut encoder = Vec::with_capacity(6);
    let mut inputs = 2;
    for (index, filters) in FILTERS.into_iter().enumerate() {
        let name = format!("{stem}/encoder{index}");
        encoder.push((
            conv(
                tensors,
                &format!("{name}/conv"),
                5,
                (inputs, filters),
                false,
            )?,
            norm(tensors, &format!("{name}/norm"), filters)?,
        ));
        inputs = filters;
    }
    let mut decoder = Vec::with_capacity(6);
    for (index, shape) in DECODER.into_iter().enumerate() {
        let name = format!("{stem}/decoder{index}");
        decoder.push((
            conv(tensors, &format!("{name}/conv"), 5, shape, true)?,
            norm(tensors, &format!("{name}/norm"), shape.1)?,
        ));
    }
    let last = conv(tensors, &format!("{stem}/last"), 4, (1, 2), false)?;
    Ok(UNet {
        encoder,
        decoder,
        last,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_reads_back_and_a_damaged_one_is_refused() {
        let mut tensors = Tensors::new();
        tensors.insert(
            "a/b".to_owned(),
            Tensor {
                dims: vec![2, 3],
                values: vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.5],
            },
        );
        tensors.insert(
            "c".to_owned(),
            Tensor {
                dims: vec![],
                values: vec![7.0],
            },
        );
        let bytes = write(&tensors);
        assert_eq!(read(&bytes), Ok(tensors));
        assert_eq!(read(&bytes[..bytes.len() - 1]), Err(WeightsError::Format));
        let mut extra = bytes.clone();
        extra.push(0);
        assert_eq!(read(&extra), Err(WeightsError::Format));
        assert_eq!(read(b"PLTRWGT0\0\0\0\0"), Err(WeightsError::Format));
        // A count far past what the file holds is refused before anything is allocated.
        let mut huge = MAGIC.to_vec();
        huge.extend_from_slice(&1u32.to_le_bytes());
        huge.extend_from_slice(&1u16.to_le_bytes());
        huge.push(b'x');
        huge.push(2);
        huge.extend_from_slice(&u32::MAX.to_le_bytes());
        huge.extend_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(read(&huge), Err(WeightsError::Format));
    }

    #[test]
    fn a_network_needs_every_tensor_with_its_shape() {
        let tensors = Tensors::new();
        assert_eq!(
            unet(&tensors, "vocals").err(),
            Some(WeightsError::Missing(
                "vocals/encoder0/conv/kernel".to_owned()
            ))
        );
    }
}
