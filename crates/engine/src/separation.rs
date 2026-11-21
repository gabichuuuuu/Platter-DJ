//! Separating a track into four stem files, on the thread that calls it: decoding, the change
//! of rate to that of the model, the separation and four WAV files of 16 bits at the rate of
//! the model. A stem deck brings them to the rate of the track when it loads them.
//!
//! Behaviour note: `docs/behavior/stems.md`, *Separation*; the choices:
//! `docs/decisions/0038-stem-separation.md`.

use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use platter_core::{SampleRate, Stem};
use platter_decode::decode_file;
use platter_dsp::interpolate::cubic;
use platter_stems::{SeparationError, Separator};

/// Why a separation gave no stems.
#[derive(Debug)]
pub enum SeparationFailure {
    /// The track cannot be decoded.
    Decode(String),
    /// A file of the stems cannot be written.
    Write(std::io::Error),
    Model(SeparationError),
}

impl std::fmt::Display for SeparationFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Decode(why) => write!(formatter, "the track cannot be read: {why}"),
            Self::Write(error) => write!(formatter, "the stems cannot be written: {error}"),
            Self::Model(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for SeparationFailure {}

/// Interleaved stereo at `from` brought to `to` with cubic interpolation. Going down, what is
/// above the new Nyquist frequency folds back; the model works below 11 kHz, far under it.
pub fn resample(stereo: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || from == 0 || to == 0 {
        return stereo.to_vec();
    }
    let frames = stereo.len() / 2;
    let out_frames = (frames as u64 * u64::from(to) / u64::from(from)) as usize;
    let step = f64::from(from) / f64::from(to);
    let at = |index: i64, side: usize| {
        usize::try_from(index)
            .ok()
            .and_then(|index| stereo.get(index * 2 + side))
            .copied()
            .unwrap_or(0.0)
    };
    let mut out = Vec::with_capacity(out_frames * 2);
    for frame in 0..out_frames {
        let position = frame as f64 * step;
        let base = position.floor() as i64;
        let fraction = (position - position.floor()) as f32;
        for side in 0..2 {
            out.push(cubic(
                [-1, 0, 1, 2].map(|offset| at(base + offset, side)),
                fraction,
            ));
        }
    }
    out
}

/// A WAV file of 16-bit samples, written as the stems come.
struct Pcm16 {
    out: BufWriter<File>,
    bytes: u64,
}

impl Pcm16 {
    fn create(path: &Path, rate: u32) -> std::io::Result<Self> {
        let mut out = BufWriter::new(File::create(path)?);
        out.write_all(b"RIFF\0\0\0\0WAVEfmt ")?;
        out.write_all(&16u32.to_le_bytes())?;
        out.write_all(&1u16.to_le_bytes())?;
        out.write_all(&2u16.to_le_bytes())?;
        out.write_all(&rate.to_le_bytes())?;
        out.write_all(&(rate * 4).to_le_bytes())?;
        out.write_all(&4u16.to_le_bytes())?;
        out.write_all(&16u16.to_le_bytes())?;
        out.write_all(b"data\0\0\0\0")?;
        Ok(Self { out, bytes: 0 })
    }

    fn write(&mut self, samples: &[f32]) -> std::io::Result<()> {
        for sample in samples {
            let value = (sample.clamp(-1.0, 1.0) * 32767.0).round() as i16;
            self.out.write_all(&value.to_le_bytes())?;
        }
        self.bytes += samples.len() as u64 * 2;
        if self.bytes > u64::from(u32::MAX - 44) {
            return Err(std::io::Error::other("the stem does not fit in a WAV file"));
        }
        Ok(())
    }

    fn finish(mut self) -> std::io::Result<()> {
        let bytes = self.bytes as u32;
        self.out.seek(SeekFrom::Start(4))?;
        self.out.write_all(&(36 + bytes).to_le_bytes())?;
        self.out.seek(SeekFrom::Start(40))?;
        self.out.write_all(&bytes.to_le_bytes())?;
        self.out.flush()
    }
}

/// Separates the track at `path` into four WAV files in `folder`, named after the stems, and
/// gives their paths in the order of [`Stem::ALL`]. `progress` gets the share done and stops
/// the work when it returns `false`; files half written are removed.
pub fn separate_track(
    path: &Path,
    folder: &Path,
    model: &dyn Separator,
    progress: &mut dyn FnMut(f32) -> bool,
) -> Result<[PathBuf; Stem::COUNT], SeparationFailure> {
    let decoded =
        decode_file(path).map_err(|error| SeparationFailure::Decode(error.to_string()))?;
    let rate: SampleRate = decoded.sample_rate;
    let stereo = resample(&decoded.into_stereo(), rate.hz(), model.rate());
    std::fs::create_dir_all(folder).map_err(SeparationFailure::Write)?;
    let paths = Stem::ALL.map(|stem| folder.join(format!("{}.wav", stem.name())));
    let outcome = write_stems(&stereo, &paths, model, progress);
    if outcome.is_err() {
        for path in &paths {
            let _ = std::fs::remove_file(path);
        }
    }
    outcome.map(|()| paths)
}

fn write_stems(
    stereo: &[f32],
    paths: &[PathBuf; Stem::COUNT],
    model: &dyn Separator,
    progress: &mut dyn FnMut(f32) -> bool,
) -> Result<(), SeparationFailure> {
    let mut files = Vec::with_capacity(Stem::COUNT);
    for path in paths {
        files.push(Pcm16::create(path, model.rate()).map_err(SeparationFailure::Write)?);
    }
    // The sink and the progress both look at the first failure to write.
    let failure: std::cell::RefCell<Option<std::io::Error>> = std::cell::RefCell::new(None);
    let outcome = model.separate(
        stereo,
        &mut |stem, samples| {
            let mut failed = failure.borrow_mut();
            if failed.is_none()
                && let Some(file) = files.get_mut(stem.index())
                && let Err(error) = file.write(samples)
            {
                *failed = Some(error);
            }
        },
        &mut |done| failure.borrow().is_none() && progress(done),
    );
    let failure = failure.into_inner();
    if let Err(error) = outcome {
        return Err(match failure {
            Some(error) => SeparationFailure::Write(error),
            None => SeparationFailure::Model(error),
        });
    }
    if let Some(error) = failure {
        return Err(SeparationFailure::Write(error));
    }
    for file in files {
        file.finish().map_err(SeparationFailure::Write)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sine_keeps_its_frequency_through_a_change_of_rate() {
        let from = 48_000;
        let stereo: Vec<f32> = (0..from * 2)
            .map(|i| (2.0 * std::f32::consts::PI * 1000.0 * (i / 2) as f32 / from as f32).sin())
            .collect();
        let out = resample(&stereo, from as u32, 44_100);
        assert_eq!(out.len(), 44_100 * 2);
        // At 44.1 kHz the sine crosses zero upward 1000 times a second.
        let frames = out.as_chunks::<2>().0;
        let crossings = frames
            .iter()
            .zip(frames.iter().skip(1))
            .filter(|(a, b)| a[0] < 0.0 && b[0] >= 0.0)
            .count();
        assert!((999..=1001).contains(&crossings), "{crossings}");
        assert_eq!(resample(&stereo, 48_000, 48_000).len(), stereo.len());
    }

    /// A model that gives every stem a quarter of the mixture.
    struct Quarter;

    impl Separator for Quarter {
        fn rate(&self) -> u32 {
            44_100
        }

        fn separate(
            &self,
            stereo: &[f32],
            sink: &mut dyn FnMut(Stem, &[f32]),
            progress: &mut dyn FnMut(f32) -> bool,
        ) -> Result<(), SeparationError> {
            let quarter: Vec<f32> = stereo.iter().map(|sample| sample / 4.0).collect();
            for half in quarter.chunks(quarter.len().div_ceil(2).max(1)) {
                if !progress(0.5) {
                    return Err(SeparationError::Cancelled);
                }
                for stem in Stem::ALL {
                    sink(stem, half);
                }
            }
            Ok(())
        }
    }

    fn track(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join("platter-tests").join(name);
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("track.wav");
        let rate = SampleRate::new(48_000).unwrap();
        let mut writer = crate::WavWriter::new(File::create(&path).unwrap(), rate, 2).unwrap();
        writer.write(&vec![0.5; 48_000 * 2]).unwrap();
        writer.finish().unwrap();
        path
    }

    #[test]
    fn a_track_becomes_four_wav_files_at_the_rate_of_the_model() {
        let path = track("separate-track");
        let folder = path.with_file_name("stems");
        let paths = separate_track(&path, &folder, &Quarter, &mut |_| true).unwrap();
        for (stem, path) in Stem::ALL.into_iter().zip(&paths) {
            assert_eq!(path.file_name().unwrap(), format!("{stem}.wav").as_str());
            let audio = decode_file(path).unwrap();
            assert_eq!(audio.sample_rate.hz(), 44_100);
            assert_eq!(audio.frames(), 44_100);
            assert!((audio.samples[1000] - 0.125).abs() < 1e-3);
        }
    }

    #[test]
    fn a_cancelled_separation_leaves_no_file() {
        let path = track("separate-cancel");
        let folder = path.with_file_name("stems");
        let outcome = separate_track(&path, &folder, &Quarter, &mut |_| false);
        assert!(matches!(
            outcome,
            Err(SeparationFailure::Model(SeparationError::Cancelled))
        ));
        assert!(!folder.join("drums.wav").exists());
    }
}
