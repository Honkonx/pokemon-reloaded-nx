use crate::audio_src::{ChannelCount, Sample, SampleRate, Source};
use nanomp3_core::{Decoder, MAX_SAMPLES_PER_FRAME};
use std::sync::Arc;

/// This MP3 player is deliberately designed to emulate bugs in GameMaker 8. Specifically:
/// - the sample rate from the first frame is assumed to be the sample rate of the whole file
/// - any frames with a different channel count than the first frame are skipped immediately
///
/// Decoding uses `nanomp3-core` (a pure Rust, `no_std` port of minimp3) instead of the C `rmp3`
/// wrapper the original used.
#[derive(Clone)]
pub struct Mp3Player {
    file: Arc<[u8]>,
    channels: ChannelCount,
    sample_rate: SampleRate,
    length: usize, // Pre-calculated number of samples that will actually be output by GM8
    decoder: Decoder,
    offset: usize,
    buffer: Box<[Sample; MAX_SAMPLES_PER_FRAME]>,
    buffer_off: usize,
    buffer_len: usize,
}

#[derive(Debug)]
pub enum Error {
    InvalidFile,
    InvalidDetails,
    NoDetails,
}

/// One frame as the player cares about it.
enum Frame {
    Audio { channels: u16, sample_rate: u32, samples_per_channel: usize },
    Other,
}

/// Decodes the next frame at the start of `data` into `pcm`; `None` at the end of the data.
fn next_frame(decoder: &mut Decoder, data: &[u8], pcm: &mut [Sample]) -> Option<(Frame, usize)> {
    if data.is_empty() {
        return None
    }
    let (consumed, result) = decoder.decode(data, pcm);
    if consumed == 0 {
        return None
    }
    let frame = match result {
        Ok(info) if info.samples_produced > 0 => Frame::Audio {
            channels: u16::from(info.channels.num()),
            sample_rate: info.sample_rate,
            samples_per_channel: info.samples_produced,
        },
        _ => Frame::Other,
    };
    Some((frame, consumed))
}

impl Mp3Player {
    pub fn new(file: impl Into<Vec<u8>>) -> Result<Self, Error> {
        let mut file: Vec<u8> = file.into();
        if let Some(payload) = super::mp3_wave_payload(&file) {
            let length = payload.len();
            file.copy_within(payload, 0);
            file.truncate(length);
        }
        let mut scratch = Box::new([0.0 as Sample; MAX_SAMPLES_PER_FRAME]);

        // First audio frame fixes the format for the whole file.
        let (channels, sample_rate) = {
            let mut decoder = Decoder::new();
            let mut offset = 0;
            let mut found = None;
            while let Some((frame, consumed)) = next_frame(&mut decoder, &file[offset..], &mut scratch[..]) {
                offset += consumed;
                if let Frame::Audio { channels, sample_rate, .. } = frame {
                    found = Some((channels, sample_rate));
                    break
                }
            }
            found.ok_or(Error::NoDetails)?
        };

        let mut length = 0;
        let mut decoder = Decoder::new();
        let mut offset = 0;
        while let Some((frame, consumed)) = next_frame(&mut decoder, &file[offset..], &mut scratch[..]) {
            offset += consumed;
            if let Frame::Audio { channels: c, samples_per_channel, .. } = frame {
                if c == channels {
                    length += samples_per_channel;
                }
            }
        }

        Ok(Self {
            file: file.into(),
            channels: ChannelCount::new(channels).ok_or(Error::InvalidDetails)?,
            sample_rate: SampleRate::new(sample_rate).ok_or(Error::InvalidDetails)?,
            decoder: Decoder::new(),
            length,
            offset: 0,
            buffer: scratch,
            buffer_off: 0,
            buffer_len: 0,
        })
    }

    /// The number of samples which will actually be played out. Divide by sample rate to get length in seconds.
    #[inline(always)]
    pub fn length(&self) -> usize {
        self.length
    }

    fn flush(&mut self, output: &mut [Sample]) -> usize {
        // get the biggest slice that can be copied directly into `output`
        let mut buffer = &self.buffer[self.buffer_off..self.buffer_off + self.buffer_len];
        if buffer.len() > output.len() {
            buffer = &buffer[..output.len()];
        }

        // copy samples into `output`
        (&mut output[..buffer.len()]).copy_from_slice(buffer);

        // adjust internal state
        self.buffer_len -= buffer.len();
        if self.buffer_len > 0 {
            self.buffer_off += buffer.len();
        } else {
            self.buffer_off = 0;
        }

        // return samples written
        buffer.len()
    }

    fn refill(&mut self) -> bool {
        loop {
            match next_frame(&mut self.decoder, &self.file[self.offset..], &mut self.buffer[..]) {
                Some((Frame::Audio { channels, samples_per_channel, .. }, consumed)) => {
                    self.offset += consumed;
                    if self.channels.get() == channels {
                        self.buffer_off = 0;
                        self.buffer_len = usize::from(channels) * samples_per_channel;
                        break true
                    }
                },
                Some((Frame::Other, consumed)) => self.offset += consumed,
                None => {
                    self.buffer_off = 0;
                    self.buffer_len = 0;
                    break false
                },
            }
        }
    }
}

impl Source for Mp3Player {
    #[inline(always)]
    fn channel_count(&self) -> ChannelCount {
        self.channels
    }

    #[inline(always)]
    fn sample_rate(&self) -> SampleRate {
        self.sample_rate
    }

    fn write_samples(&mut self, mut buffer: &mut [Sample]) -> usize {
        let mut samples_written = 0usize;
        loop {
            let flushed = self.flush(buffer);
            if buffer.len() == flushed {
                break samples_written + flushed
            }
            buffer = &mut buffer[flushed..];
            samples_written += flushed;

            if self.buffer_len == 0 {
                if !self.refill() {
                    break samples_written
                }
            }
        }
    }

    #[inline(always)]
    fn reset(&mut self) {
        self.buffer_off = 0;
        self.buffer_len = 0;
        self.offset = 0;
        self.decoder = Decoder::new();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn garbage_is_not_an_mp3() {
        assert!(matches!(Mp3Player::new(vec![0u8; 4096]), Err(Error::NoDetails)));
        assert!(matches!(Mp3Player::new(Vec::new()), Err(Error::NoDetails)));
    }

    #[test]
    fn generated_mp3_in_wave_decodes_finite_stereo_and_resets() {
        let file = crate::title_tests::fixture::mp3_wave_with_metadata();
        assert!(super::super::wav_is_mp3(&file));
        assert!(super::super::check_wav(file.clone().into_boxed_slice()).is_ok());
        let mut player = Mp3Player::new(file).unwrap();
        assert_eq!(player.sample_rate().get(), 44_100);
        assert_eq!(player.channel_count().get(), 2);
        assert_eq!(player.length(), 3 * 1152);
        let mut buffer = vec![f32::NAN; 257];
        let mut samples = Vec::new();
        loop {
            let count = player.write_samples(&mut buffer);
            if count == 0 { break; }
            assert!(buffer[..count].iter().all(|sample| sample.is_finite()));
            samples.extend_from_slice(&buffer[..count]);
        }
        assert_eq!(samples.len(), 3 * 1152 * 2);
        assert!(samples.iter().all(|sample| *sample == 0.0), "original frames encode silence");
        player.reset();
        assert_eq!(player.write_samples(&mut buffer), buffer.len());
        assert_eq!(&buffer, &samples[..buffer.len()]);
    }
}
