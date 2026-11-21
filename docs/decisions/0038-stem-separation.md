# 0038. Stem separation: own inference over weights the user downloads

Status: accepted (M4).

## Context

M4 separates a track into four stems (`docs/behavior/stems.md`, *Separation*). The roadmap
decided that a model runs through ONNX, is a separate download and is never trained on data
restricted to non-commercial use. On 21 November 2026 the open models were reviewed at their
sources: Demucs, Open-Unmix and the RoFormer and SCNet checkpoints are trained on MUSDB18, whose
licence is for education only, or publish no weights; only the 4-stem Spleeter model of Deezer is
trained on other data (the internal data of Deezer, among it the Bean set). The paper of its
authors puts its pre-trained models under the MIT licence
(<https://doi.org/10.21105/joss.02154>); its repository puts only the code under it, and the
question of the weights is open without an answer. `docs/legal-notes.md` already said that,
without a confirmation, Platter neither hosts nor packages the model and the user downloads it
from the original publication.

The crate `ort` (ONNX Runtime) was approved for this. At its release 2.0.0-rc.15 it has no
stable version; by default it downloads a prebuilt library from the server of its authors at
build time; the library is a binary of Microsoft to ship beside Platter, with telemetry on unless
it is turned off, and with no prebuilt build for macOS on Intel. The only ONNX export of Spleeter
is a repack by a third party, not the original publication.

## Options

- **ONNX Runtime with an ONNX export.** Fast, but brings a native library to ship and keep, a
  build that reaches the network unless configured otherwise, and a model file that is not the
  original publication.
- **ONNX Runtime with a conversion made by Platter** from the original checkpoint into ONNX. Keeps
  the original publication but still needs the native library.
- **Own inference.** The network of Spleeter is small and fully described by its code: per stem a
  U-Net of twelve layers of 5 by 5 convolutions with a stride of two, batch normalisation and ELU,
  over 512 frames by 1024 bins of the magnitude spectrum, then soft masks with an exponent of two.
  About six thousand million multiply-adds per stem for each 11.9 s of audio. Written in Rust it
  needs no `unsafe`, no native library and no network, and reads the original file.

## Choice

- `platter-stems` (L1) holds the inference: the transform, the networks, the masks, the reader
  of the original archive (gzip, tar and the tensor bundle of TensorFlow, each bounded) and the
  file of weights of Platter. No external dependency; `ort` is not used.
- A trait, `Separator`, stands between the application and the network, and the weights live in
  a file format of Platter, so that a model trained later on data Platter may use takes the place
  of Spleeter without touching the rest.
- The user downloads `4stems.tar.gz` of release v1.4.0. Platter checks its SHA-256 against the one
  the release publishes, `3adb4a50ad4eb18c7c4d65fcf4cf2367a07d48408a5eb7d03cd20067429dfaa8`, and
  refuses anything else.
- The engine (L2) runs a separation on a worker: decoding, the change of rate to 44.1 kHz, the
  separation and four WAV files of 16 bits at 44.1 kHz, which a stem deck brings to the rate of
  the track when it loads them. The application attaches them.
- The weights are never in the repository nor in CI: the tests run the networks on small
  invented weights, and the reader on small archives the tests build.

## How it was measured

- The networks, the transform and the masks are checked on invented weights: a convolution of
  stride two pads as TensorFlow does, a transposed convolution is the transpose of the forward one,
  and four equal masks give each stem a quarter of the mixture, sample for sample.
- The readers are checked on files the tests build: gzip of Python 3 with fixed and dynamic codes,
  tar, and a checkpoint laid out as TensorFlow lays it out with the layer names of Keras.
- Speed, with invented weights, whose work is that of the real ones
  (`cargo test -p platter-stems --release speed -- --ignored`): on 21 November 2026, Windows x64,
  20 threads, 30 s of audio in 3.0 s. The application separates on half the processors.
- Not yet: the real archive read end to end and a separation compared by ear and against the
  original implementation.
