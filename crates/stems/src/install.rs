//! Installing the 4-stem Spleeter model from its original archive into a file of weights of
//! Platter, and loading that file.
//!
//! The archive is the one of release v1.4.0 of Deezer, checked against the digest the release
//! publishes in its `checksum.json`. Inside it, the checkpoint names its layers as Keras did when
//! it built them: for each stem, in the order vocals, drums, bass, other, seven convolutions
//! (`conv2d`, `conv2d_1`, …: six of the encoder and the last), six transposed convolutions and
//! twelve batch normalisations, numbered on across the stems. Choices:
//! `docs/decisions/0038-stem-separation.md`.

use std::collections::BTreeMap;

use platter_core::Stem;

use crate::bundle::{BundleError, checkpoint, untar};
use crate::inflate::gunzip;
use crate::sha256::hex_digest;
use crate::spleeter::Spleeter;
use crate::weights::{self, Tensor, Tensors, WeightsError};

/// Where the user downloads the model, with an action of their own.
pub const SPLEETER_URL: &str =
    "https://github.com/deezer/spleeter/releases/download/v1.4.0/4stems.tar.gz";
/// The SHA-256 of that file, as `checksum.json` of the release gives it.
pub const SPLEETER_SHA256: &str =
    "3adb4a50ad4eb18c7c4d65fcf4cf2367a07d48408a5eb7d03cd20067429dfaa8";
/// More than the archive holds once inflated.
const MAX_ARCHIVE_BYTES: usize = 512 * 1024 * 1024;
/// The order of the networks in the checkpoint.
const MODEL_ORDER: [Stem; 4] = [Stem::Vocals, Stem::Drums, Stem::Bass, Stem::Other];

#[derive(Debug, PartialEq, Eq)]
pub enum InstallError {
    /// The file is not the archive of the release: its digest differs.
    NotTheModel,
    /// The archive does not hold the checkpoint it should.
    Damaged(String),
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotTheModel => {
                formatter.write_str("the file is not 4stems.tar.gz of release v1.4.0 of Spleeter")
            }
            Self::Damaged(why) => write!(formatter, "the archive of the model is damaged: {why}"),
        }
    }
}

impl std::error::Error for InstallError {}

impl From<BundleError> for InstallError {
    fn from(error: BundleError) -> Self {
        Self::Damaged(error.0.to_owned())
    }
}

impl From<WeightsError> for InstallError {
    fn from(error: WeightsError) -> Self {
        Self::Damaged(error.to_string())
    }
}

/// The number at the end of a layer name, `conv2d` being 0 and `conv2d_7` being 7; `None`
/// for another kind of layer.
fn layer_number(name: &str, kind: &str) -> Option<usize> {
    let layer = name.rsplit('/').nth(1)?;
    if layer == kind {
        return Some(0);
    }
    layer.strip_prefix(kind)?.strip_prefix('_')?.parse().ok()
}

/// The layers of one kind, by their number, as `(number, variable, tensor)`.
fn layers<'a>(tensors: &'a Tensors, kind: &str) -> BTreeMap<usize, BTreeMap<String, &'a Tensor>> {
    let mut found: BTreeMap<usize, BTreeMap<String, &Tensor>> = BTreeMap::new();
    for (name, tensor) in tensors {
        if let Some(number) = layer_number(name, kind) {
            let variable = name.rsplit('/').next().unwrap_or_default().to_owned();
            found.entry(number).or_default().insert(variable, tensor);
        }
    }
    found
}

fn take<'a>(
    layer: Option<&BTreeMap<String, &'a Tensor>>,
    variable: &str,
) -> Result<&'a Tensor, InstallError> {
    layer
        .and_then(|layer| layer.get(variable))
        .copied()
        .ok_or_else(|| InstallError::Damaged(format!("a layer has no {variable}")))
}

/// The tensors of the checkpoint renamed into those of the file of Platter.
fn rename(tensors: &Tensors) -> Result<Tensors, InstallError> {
    let convs: Vec<_> = layers(tensors, "conv2d").into_values().collect();
    let transposed: Vec<_> = layers(tensors, "conv2d_transpose").into_values().collect();
    let norms: Vec<_> = layers(tensors, "batch_normalization")
        .into_values()
        .collect();
    if convs.len() != 28 || transposed.len() != 24 || norms.len() != 48 {
        return Err(InstallError::Damaged(format!(
            "{} convolutions, {} transposed convolutions and {} normalisations instead of 28, 24 and 48",
            convs.len(),
            transposed.len(),
            norms.len()
        )));
    }
    let mut out = Tensors::new();
    for (index, stem) in MODEL_ORDER.into_iter().enumerate() {
        let stem = stem.name();
        let mut put = |name: String, tensor: &Tensor| {
            out.insert(name, tensor.clone());
        };
        for layer in 0..7 {
            let source = convs.get(index * 7 + layer);
            let name = if layer < 6 {
                format!("{stem}/encoder{layer}/conv")
            } else {
                format!("{stem}/last")
            };
            put(format!("{name}/kernel"), take(source, "kernel")?);
            put(format!("{name}/bias"), take(source, "bias")?);
        }
        for layer in 0..6 {
            let source = transposed.get(index * 6 + layer);
            let kernel = take(source, "kernel")?;
            // Keras keeps the kernel of a transposed convolution as [row][column][output][input].
            let [rows, columns, outputs, inputs] = kernel.dims[..] else {
                return Err(InstallError::Damaged(
                    "a transposed kernel is not of rank 4".to_owned(),
                ));
            };
            let mut values = vec![0.0; kernel.values.len()];
            for position in 0..rows * columns {
                for output in 0..outputs {
                    for input in 0..inputs {
                        values[(position * inputs + input) * outputs + output] =
                            kernel.values[(position * outputs + output) * inputs + input];
                    }
                }
            }
            let name = format!("{stem}/decoder{layer}/conv");
            put(
                format!("{name}/kernel"),
                &Tensor {
                    dims: vec![rows, columns, inputs, outputs],
                    values,
                },
            );
            put(format!("{name}/bias"), take(source, "bias")?);
        }
        for layer in 0..12 {
            let source = norms.get(index * 12 + layer);
            let name = if layer < 6 {
                format!("{stem}/encoder{layer}/norm")
            } else {
                format!("{stem}/decoder{}/norm", layer - 6)
            };
            for (from, to) in [
                ("gamma", "gamma"),
                ("beta", "beta"),
                ("moving_mean", "mean"),
                ("moving_variance", "variance"),
            ] {
                put(format!("{name}/{to}"), take(source, from)?);
            }
        }
    }
    Ok(out)
}

/// The file of weights of Platter made from the original archive of the model.
pub fn install_spleeter(archive: &[u8]) -> Result<Vec<u8>, InstallError> {
    if hex_digest(archive) != SPLEETER_SHA256 {
        return Err(InstallError::NotTheModel);
    }
    convert(archive)
}

/// [`install_spleeter`] without the check of the digest, for archives the tests build.
fn convert(archive: &[u8]) -> Result<Vec<u8>, InstallError> {
    let tar = gunzip(archive, MAX_ARCHIVE_BYTES)
        .map_err(|_| InstallError::Damaged("it cannot be inflated".to_owned()))?;
    let files = untar(&tar)?;
    let index = files
        .iter()
        .find(|(name, _)| name.ends_with(".index"))
        .map(|(name, bytes)| (name.trim_end_matches(".index").to_owned(), *bytes))
        .ok_or_else(|| InstallError::Damaged("it holds no index".to_owned()))?;
    let data = files
        .get(&format!("{}.data-00000-of-00001", index.0))
        .ok_or_else(|| InstallError::Damaged("it holds no data".to_owned()))?;
    // Only the weights the network needs, not the state of the training.
    let wanted = |name: &str| {
        !name.contains("Adam")
            && [
                "kernel",
                "bias",
                "gamma",
                "beta",
                "moving_mean",
                "moving_variance",
            ]
            .iter()
            .any(|variable| name.ends_with(&format!("/{variable}")))
    };
    let tensors = rename(&checkpoint(index.1, data, wanted)?)?;
    // The file has to make the four networks.
    load_tensors(&tensors, 1)?;
    Ok(weights::write(&tensors))
}

fn load_tensors(tensors: &Tensors, threads: usize) -> Result<Spleeter, WeightsError> {
    let nets = [
        weights::unet(tensors, Stem::Vocals.name())?,
        weights::unet(tensors, Stem::Drums.name())?,
        weights::unet(tensors, Stem::Bass.name())?,
        weights::unet(tensors, Stem::Other.name())?,
    ];
    Spleeter::new(nets, threads).ok_or(WeightsError::Format)
}

/// The model of a file of weights of Platter, running on `threads` threads.
pub fn load_spleeter(bytes: &[u8], threads: usize) -> Result<Spleeter, WeightsError> {
    load_tensors(&weights::read(bytes)?, threads)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::tests::{checkpoint_of, tar_of};
    use crate::net::FILTERS;
    use crate::weights::DECODER;

    /// The tensors of a checkpoint named as Keras names them, with small values.
    pub(super) fn keras_checkpoint_pub() -> Vec<(String, Vec<usize>, Vec<f32>)> {
        keras_checkpoint()
    }

    fn keras_checkpoint() -> Vec<(String, Vec<usize>, Vec<f32>)> {
        let mut tensors = Vec::new();
        let filled = |dims: Vec<usize>, seed: usize| {
            let count: usize = dims.iter().product();
            let values = (0..count)
                .map(|i| ((i * 31 + seed * 7) % 17) as f32 / 170.0)
                .collect();
            (dims, values)
        };
        let suffix = |number: usize| {
            if number == 0 {
                String::new()
            } else {
                format!("_{number}")
            }
        };
        for stem in 0..4 {
            let mut inputs = 2;
            for (layer, filters) in FILTERS.into_iter().enumerate() {
                let n = stem * 7 + layer;
                let (dims, values) = filled(vec![5, 5, inputs, filters], n);
                tensors.push((format!("conv2d{}/kernel", suffix(n)), dims, values));
                tensors.push((
                    format!("conv2d{}/bias", suffix(n)),
                    vec![filters],
                    vec![0.0; filters],
                ));
                inputs = filters;
            }
            let n = stem * 7 + 6;
            tensors.push((
                format!("conv2d{}/kernel", suffix(n)),
                vec![4, 4, 1, 2],
                vec![0.01; 32],
            ));
            tensors.push((format!("conv2d{}/bias", suffix(n)), vec![2], vec![0.0; 2]));
            for (layer, (inputs, outputs)) in DECODER.into_iter().enumerate() {
                let n = stem * 6 + layer;
                let (dims, values) = filled(vec![5, 5, outputs, inputs], n);
                tensors.push((
                    format!("conv2d_transpose{}/kernel", suffix(n)),
                    dims,
                    values,
                ));
                tensors.push((
                    format!("conv2d_transpose{}/bias", suffix(n)),
                    vec![outputs],
                    vec![0.0; outputs],
                ));
            }
            let channels = FILTERS
                .into_iter()
                .chain(DECODER.map(|(_, outputs)| outputs));
            for (layer, channels) in channels.enumerate() {
                let n = stem * 12 + layer;
                let name = format!("batch_normalization{}", suffix(n));
                tensors.push((format!("{name}/gamma"), vec![channels], vec![1.0; channels]));
                tensors.push((format!("{name}/beta"), vec![channels], vec![0.0; channels]));
                tensors.push((
                    format!("{name}/moving_mean"),
                    vec![channels],
                    vec![0.0; channels],
                ));
                tensors.push((
                    format!("{name}/moving_variance"),
                    vec![channels],
                    vec![1.0; channels],
                ));
                // The state of the optimiser, which the installation leaves out.
                tensors.push((
                    format!("{name}/gamma/Adam"),
                    vec![channels],
                    vec![9.0; channels],
                ));
            }
        }
        tensors
    }

    /// A gzip file of stored blocks around `content`.
    fn gzip_of(content: &[u8]) -> Vec<u8> {
        let mut out = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 0xff];
        let mut chunks = content.chunks(65_535).peekable();
        while let Some(chunk) = chunks.next() {
            out.push(u8::from(chunks.peek().is_none()));
            let length = chunk.len() as u16;
            out.extend_from_slice(&length.to_le_bytes());
            out.extend_from_slice(&(!length).to_le_bytes());
            out.extend_from_slice(chunk);
        }
        out.extend_from_slice(&[0, 0, 0, 0]);
        out.extend_from_slice(&(content.len() as u32).to_le_bytes());
        out
    }

    #[test]
    fn an_archive_of_the_model_becomes_four_networks_of_platter() {
        let keras = keras_checkpoint();
        let borrowed: Vec<(&str, Vec<usize>, Vec<f32>)> = keras
            .iter()
            .map(|(name, dims, values)| (name.as_str(), dims.clone(), values.clone()))
            .collect();
        let (index, data) = checkpoint_of(&borrowed);
        let archive = gzip_of(&tar_of(&[
            ("4stems/checkpoint", b"model_checkpoint_path: \"model\""),
            ("4stems/model.index", &index),
            ("4stems/model.data-00000-of-00001", &data),
        ]));
        assert_eq!(install_spleeter(&archive), Err(InstallError::NotTheModel));
        let file = convert(&archive).unwrap();
        let tensors = weights::read(&file).unwrap();
        // The transposed kernel of the second network now reads [row][column][input][output].
        let kernel = &tensors["drums/decoder0/conv/kernel"];
        assert_eq!(kernel.dims, vec![5, 5, 512, 256]);
        let original = &keras
            .iter()
            .find(|(name, ..)| name == "conv2d_transpose_6/kernel")
            .unwrap()
            .2;
        assert_eq!(kernel.values[3 * 256 + 2], original[2 * 512 + 3]);
        assert!(!tensors.keys().any(|name| name.contains("Adam")));
        assert!(load_spleeter(&file, 2).is_ok());
    }

    #[test]
    fn layer_names_give_their_numbers() {
        assert_eq!(layer_number("conv2d/kernel", "conv2d"), Some(0));
        assert_eq!(layer_number("conv2d_13/bias", "conv2d"), Some(13));
        assert_eq!(layer_number("conv2d_transpose_2/bias", "conv2d"), None);
        assert_eq!(
            layer_number("conv2d_transpose_2/bias", "conv2d_transpose"),
            Some(2)
        );
    }
}

/// How long a separation takes, with invented weights: the work is that of the real ones.
/// `cargo test -p platter-stems --release speed -- --ignored --nocapture`; 21 November 2026,
/// Windows x64, 20 threads: 30 s of audio in 3.0 s.
#[cfg(test)]
mod speed {
    use super::*;
    use crate::Separator;

    #[test]
    #[ignore]
    fn speed_of_thirty_seconds() {
        let keras = super::tests::keras_checkpoint_pub();
        let borrowed: Vec<(&str, Vec<usize>, Vec<f32>)> = keras
            .iter()
            .map(|(name, dims, values)| (name.as_str(), dims.clone(), values.clone()))
            .collect();
        let (index, data) = crate::bundle::tests::checkpoint_of(&borrowed);
        let tensors = rename(&checkpoint(&index, &data, |_| true).unwrap()).unwrap();
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
        let model = load_tensors(&tensors, threads).unwrap();
        let stereo: Vec<f32> = (0..30 * 44_100 * 2)
            .map(|i| ((i * 7919) % 1000) as f32 / 1000.0 - 0.5)
            .collect();
        let start = std::time::Instant::now();
        model
            .separate(&stereo, &mut |_, _| {}, &mut |_| true)
            .unwrap();
        eprintln!("threads {threads}: 30 s of audio in {:?}", start.elapsed());
    }
}
