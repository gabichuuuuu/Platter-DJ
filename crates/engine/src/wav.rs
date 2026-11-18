//! Writer of WAV files with 32-bit float samples, for the deviceless render and the mix
//! recorder.
//!
//! Float samples keep the output of the engine bit for bit, which is what the determinism
//! test compares. Format: RIFF WAVE with format tag 3 (IEEE float), which requires a `fact`
//! chunk (<https://www.mmsp.ece.mcgill.ca/Documents/AudioFormats/WAVE/WAVE.html>).

use std::io::{Error, ErrorKind, Result, Seek, SeekFrom, Write};

use platter_core::SampleRate;

const HEADER_BYTES: u32 = 56;
/// A RIFF file stores its size in 32 bits.
const MAX_DATA_BYTES: u64 = (u32::MAX - HEADER_BYTES) as u64;
const IEEE_FLOAT: u16 = 3;

pub struct WavWriter<W: Write + Seek> {
    out: W,
    channels: u16,
    data_bytes: u64,
}

impl<W: Write + Seek> WavWriter<W> {
    /// Writes the header. The sizes in it stay provisional until [`WavWriter::finish`].
    pub fn new(mut out: W, rate: SampleRate, channels: u16) -> Result<Self> {
        if channels == 0 {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "a WAV file needs a channel",
            ));
        }
        let block_align = channels * 4;
        out.write_all(b"RIFF\0\0\0\0WAVEfmt ")?;
        out.write_all(&16u32.to_le_bytes())?;
        out.write_all(&IEEE_FLOAT.to_le_bytes())?;
        out.write_all(&channels.to_le_bytes())?;
        out.write_all(&rate.hz().to_le_bytes())?;
        out.write_all(&(rate.hz() * u32::from(block_align)).to_le_bytes())?;
        out.write_all(&block_align.to_le_bytes())?;
        out.write_all(&32u16.to_le_bytes())?;
        out.write_all(b"fact")?;
        out.write_all(&4u32.to_le_bytes())?;
        out.write_all(&0u32.to_le_bytes())?;
        out.write_all(b"data\0\0\0\0")?;
        Ok(Self {
            out,
            channels,
            data_bytes: 0,
        })
    }

    /// Appends interleaved samples.
    pub fn write(&mut self, samples: &[f32]) -> Result<()> {
        let bytes = samples.len() as u64 * 4;
        if self.data_bytes + bytes > MAX_DATA_BYTES {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "the audio does not fit in a WAV file",
            ));
        }
        for sample in samples {
            self.out.write_all(&sample.to_le_bytes())?;
        }
        self.data_bytes += bytes;
        Ok(())
    }

    /// Fills in the sizes and returns the output.
    pub fn finish(mut self) -> Result<W> {
        // `write` keeps the total under 32 bits.
        let data_bytes = self.data_bytes as u32;
        let frames = data_bytes / (u32::from(self.channels) * 4);
        self.out.seek(SeekFrom::Start(4))?;
        self.out
            .write_all(&(HEADER_BYTES - 8 + data_bytes).to_le_bytes())?;
        self.out.seek(SeekFrom::Start(44))?;
        self.out.write_all(&frames.to_le_bytes())?;
        self.out.seek(SeekFrom::Start(52))?;
        self.out.write_all(&data_bytes.to_le_bytes())?;
        self.out.seek(SeekFrom::End(0))?;
        self.out.flush()?;
        Ok(self.out)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use platter_core::SampleRate;
    use platter_decode::{Container, decode_bytes};

    use super::WavWriter;

    #[test]
    fn the_decoder_reads_back_exactly_what_was_written() {
        let rate = SampleRate::new(48_000).unwrap();
        let samples: Vec<f32> = (0..2_000).map(|n| (n as f32 * 0.37).sin() * 0.8).collect();
        let mut writer = WavWriter::new(Cursor::new(Vec::new()), rate, 2).unwrap();
        writer.write(&samples[..600]).unwrap();
        writer.write(&samples[600..]).unwrap();
        let bytes = writer.finish().unwrap().into_inner();
        assert_eq!(bytes.len(), 56 + samples.len() * 4);

        let audio = decode_bytes(bytes, Some("wav")).unwrap();
        assert_eq!(audio.container, Container::Wav);
        assert_eq!(audio.sample_rate, rate);
        assert_eq!(audio.channels, 2);
        assert_eq!(audio.samples, samples);
    }

    #[test]
    fn an_empty_file_is_still_well_formed() {
        let rate = SampleRate::new(44_100).unwrap();
        let writer = WavWriter::new(Cursor::new(Vec::new()), rate, 2).unwrap();
        let bytes = writer.finish().unwrap().into_inner();
        assert_eq!(bytes.len(), 56);
        assert_eq!(&bytes[4..8], &48u32.to_le_bytes());
        assert_eq!(&bytes[52..56], &0u32.to_le_bytes());
    }

    #[test]
    fn refuses_a_file_without_channels() {
        let rate = SampleRate::new(44_100).unwrap();
        assert!(WavWriter::new(Cursor::new(Vec::new()), rate, 0).is_err());
    }
}
