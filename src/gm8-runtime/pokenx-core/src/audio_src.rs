//! PCM sources for the GM8 mixer: the `Source` contract, channel conversion, resampling,
//! looping and a WAV player. Written for this port (the original used `udon`, whose
//! licensing is inconsistent); the trait shape is the same so the mixer is unchanged.

use std::{
    num::{NonZeroU16, NonZeroU32},
    sync::Arc,
};

pub type ChannelCount = NonZeroU16;
pub type Sample = f32;
pub type SampleRate = NonZeroU32;

/// A source of interleaved PCM at a fixed sample rate and channel count.
pub trait Source {
    fn channel_count(&self) -> ChannelCount;
    fn sample_rate(&self) -> SampleRate;
    /// Writes the next samples contiguously from the start of `buffer` and returns how many were
    /// written; fewer than `buffer.len()` means the sound ended, after which it returns 0.
    fn write_samples(&mut self, buffer: &mut [Sample]) -> usize;
    /// Back to the start (or default state).
    fn reset(&mut self);
}

/// Converts the channel layout of a source to `out_channels` (mono is duplicated, stereo averaged).
pub struct Rechanneler<S: Source> {
    source: S,
    out_channels: ChannelCount,
    scratch: Vec<Sample>,
}

impl<S: Source> Rechanneler<S> {
    pub fn new(source: S, out_channels: ChannelCount) -> Self {
        Self { source, out_channels, scratch: Vec::new() }
    }
}

impl<S: Source> Source for Rechanneler<S> {
    fn channel_count(&self) -> ChannelCount {
        self.out_channels
    }

    fn sample_rate(&self) -> SampleRate {
        self.source.sample_rate()
    }

    fn write_samples(&mut self, buffer: &mut [Sample]) -> usize {
        let in_ch = usize::from(self.source.channel_count().get());
        let out_ch = usize::from(self.out_channels.get());
        if in_ch == out_ch {
            return self.source.write_samples(buffer)
        }
        let frames = buffer.len() / out_ch;
        self.scratch.resize(frames * in_ch, 0.0);
        let read = self.source.write_samples(&mut self.scratch);
        let got_frames = read / in_ch;
        for f in 0..got_frames {
            let input = &self.scratch[f * in_ch..(f + 1) * in_ch];
            let output = &mut buffer[f * out_ch..(f + 1) * out_ch];
            if in_ch == 1 {
                output.fill(input[0]);
            } else if out_ch == 1 {
                output[0] = input.iter().sum::<f32>() / in_ch as f32;
            } else {
                for (c, sample) in output.iter_mut().enumerate() {
                    *sample = input.get(c).copied().unwrap_or(0.0);
                }
            }
        }
        got_frames * out_ch
    }

    fn reset(&mut self) {
        self.source.reset();
    }
}

const CHUNK_FRAMES: usize = 256;

/// Linear-interpolation sample-rate converter.
pub struct Resampler<S: Source> {
    source: S,
    out_rate: SampleRate,
    channels: usize,
    /// Source frames consumed per output frame.
    step: f64,
    t: f64,
    current: Vec<Sample>,
    next: Vec<Sample>,
    chunk: Vec<Sample>,
    chunk_len: usize,
    chunk_pos: usize,
    source_ended: bool,
    primed: bool,
    finished: bool,
}

impl<S: Source> Resampler<S> {
    pub fn new(source: S, out_rate: SampleRate) -> Self {
        let channels = usize::from(source.channel_count().get());
        let step = f64::from(source.sample_rate().get()) / f64::from(out_rate.get());
        Self {
            source,
            out_rate,
            channels,
            step,
            t: 0.0,
            current: vec![0.0; channels],
            next: vec![0.0; channels],
            chunk: vec![0.0; CHUNK_FRAMES * channels],
            chunk_len: 0,
            chunk_pos: 0,
            source_ended: false,
            primed: false,
            finished: false,
        }
    }

    /// Reads one source frame into `frame`; false when the source has no more.
    fn read_frame(&mut self, frame: &mut [Sample]) -> bool {
        if self.chunk_pos + self.channels > self.chunk_len {
            if self.source_ended {
                return false
            }
            self.chunk_len = self.source.write_samples(&mut self.chunk);
            self.chunk_pos = 0;
            if self.chunk_len < self.chunk.len() {
                self.source_ended = true;
            }
            if self.chunk_len < self.channels {
                return false
            }
        }
        frame.copy_from_slice(&self.chunk[self.chunk_pos..self.chunk_pos + self.channels]);
        self.chunk_pos += self.channels;
        true
    }
}

impl<S: Source> Source for Resampler<S> {
    fn channel_count(&self) -> ChannelCount {
        self.source.channel_count()
    }

    fn sample_rate(&self) -> SampleRate {
        self.out_rate
    }

    fn write_samples(&mut self, buffer: &mut [Sample]) -> usize {
        if self.finished {
            return 0
        }
        let mut written = 0;
        while written + self.channels <= buffer.len() {
            if !self.primed {
                let mut frame = vec![0.0; self.channels];
                if !self.read_frame(&mut frame) {
                    self.finished = true;
                    break
                }
                self.current = frame;
                let mut frame = vec![0.0; self.channels];
                if !self.read_frame(&mut frame) {
                    // A one-frame sound: play it once, then stop.
                    self.next = self.current.clone();
                    self.source_ended = true;
                } else {
                    self.next = frame;
                }
                self.primed = true;
            }
            let t = self.t as f32;
            for c in 0..self.channels {
                buffer[written + c] = self.current[c] * (1.0 - t) + self.next[c] * t;
            }
            written += self.channels;
            self.t += self.step;
            while self.t >= 1.0 {
                self.t -= 1.0;
                std::mem::swap(&mut self.current, &mut self.next);
                let mut frame = std::mem::take(&mut self.next);
                frame.resize(self.channels, 0.0);
                if !self.read_frame(&mut frame) {
                    self.finished = true;
                    self.next = frame;
                    return written
                }
                self.next = frame;
            }
        }
        written
    }

    fn reset(&mut self) {
        self.source.reset();
        self.t = 0.0;
        self.chunk_len = 0;
        self.chunk_pos = 0;
        self.source_ended = false;
        self.primed = false;
        self.finished = false;
    }
}

/// Repeats a source forever.
pub struct Cycle<S: Source>(S);

impl<S: Source> Cycle<S> {
    pub fn new(source: S) -> Self {
        Self(source)
    }
}

impl<S: Source> Source for Cycle<S> {
    fn channel_count(&self) -> ChannelCount {
        self.0.channel_count()
    }

    fn sample_rate(&self) -> SampleRate {
        self.0.sample_rate()
    }

    fn write_samples(&mut self, buffer: &mut [Sample]) -> usize {
        let mut written = 0;
        while written < buffer.len() {
            let count = self.0.write_samples(&mut buffer[written..]);
            written += count;
            if written < buffer.len() {
                self.0.reset();
                // An empty sound would loop forever without producing anything.
                if count == 0 {
                    let probe = self.0.write_samples(&mut buffer[written..written + 1]);
                    if probe == 0 {
                        break
                    }
                    written += probe;
                }
            }
        }
        written
    }

    fn reset(&mut self) {
        self.0.reset();
    }
}

/// Microsoft ADPCM (`WAVE_FORMAT_ADPCM`, tag 2): GameMaker 8 plays these, and the game ships some.
struct AdpcmFormat {
    block_align: usize,
    coefficients: Vec<(i32, i32)>,
}

impl AdpcmFormat {
    fn parse(fmt: &[u8]) -> Self {
        let block_align = usize::from(u16::from_le_bytes([fmt[12], fmt[13]]));
        // cbSize(2) samplesPerBlock(2) numCoef(2) then numCoef pairs of i16.
        let mut coefficients = Vec::new();
        if fmt.len() >= 22 {
            let count = usize::from(u16::from_le_bytes([fmt[20], fmt[21]]));
            for i in 0..count {
                let at = 22 + i * 4;
                if at + 4 > fmt.len() { break }
                coefficients.push((
                    i32::from(i16::from_le_bytes([fmt[at], fmt[at + 1]])),
                    i32::from(i16::from_le_bytes([fmt[at + 2], fmt[at + 3]])),
                ));
            }
        }
        if coefficients.is_empty() {
            coefficients = vec![(256, 0), (512, -256), (0, 0), (192, 64), (240, 0), (460, -208), (392, -232)];
        }
        Self { block_align, coefficients }
    }
}

fn decode_ms_adpcm(data: &[u8], channels: usize, format: &AdpcmFormat) -> Option<Vec<Sample>> {
    const ADAPT: [i32; 16] = [230, 230, 230, 230, 307, 409, 512, 614, 768, 614, 512, 409, 307, 230, 230, 230];
    if !(1..=2).contains(&channels) { return None }
    let header = 7 * channels;
    if format.block_align <= header { return None }
    let mut out: Vec<Sample> = Vec::new();
    for block in data.chunks(format.block_align) {
        if block.len() < header { break }
        let mut predictor = [(0i32, 0i32); 2];
        let mut delta = [0i32; 2];
        let mut s1 = [0i32; 2];
        let mut s2 = [0i32; 2];
        for c in 0..channels {
            predictor[c] = *format.coefficients.get(usize::from(block[c]))?;
        }
        let word = |offset: usize| i32::from(i16::from_le_bytes([block[offset], block[offset + 1]]));
        for c in 0..channels {
            delta[c] = word(channels + 2 * c);
            s1[c] = word(3 * channels + 2 * c);
            s2[c] = word(5 * channels + 2 * c);
        }
        // The two warm-up samples come first, oldest first.
        for c in 0..channels { out.push(s2[c] as f32 / 32768.0) }
        for c in 0..channels { out.push(s1[c] as f32 / 32768.0) }
        let mut nibble_index = 0usize;
        for &byte in &block[header..] {
            for nibble in [byte >> 4, byte & 0x0F] {
                let c = nibble_index % channels;
                nibble_index += 1;
                let signed = if nibble >= 8 { i32::from(nibble) - 16 } else { i32::from(nibble) };
                let (c1, c2) = predictor[c];
                let predicted = ((s1[c] * c1 + s2[c] * c2) >> 8) + signed * delta[c];
                let sample = predicted.clamp(-32768, 32767);
                s2[c] = s1[c];
                s1[c] = sample;
                delta[c] = ((ADAPT[usize::from(nibble)] * delta[c]) >> 8).max(16);
                out.push(sample as f32 / 32768.0);
            }
        }
    }
    Some(out)
}

#[derive(Debug, PartialEq, Eq)]
pub enum WavError {
    NotRiffWave,
    MissingChunk,
    Unsupported,
}

/// Decoded WAV (PCM 8/16/24/32-bit or 32-bit float). Cloning shares the samples and restarts
/// playback from the beginning.
#[derive(Clone)]
pub struct WavPlayer {
    samples: Arc<[Sample]>,
    channels: ChannelCount,
    sample_rate: SampleRate,
    pos: usize,
}

impl WavPlayer {
    pub fn new(file: impl AsRef<[u8]>) -> Result<Self, WavError> {
        let bytes = file.as_ref();
        if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
            return Err(WavError::NotRiffWave)
        }
        let (mut format, mut data): (Option<(u16, u16, u32, u16)>, Option<&[u8]>) = (None, None);
        let mut adpcm: Option<AdpcmFormat> = None;
        let mut at = 12;
        while at + 8 <= bytes.len() {
            let id = &bytes[at..at + 4];
            let size = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
            let body = &bytes[at + 8..(at + 8 + size).min(bytes.len())];
            match id {
                b"fmt " if body.len() >= 16 => {
                    let mut tag = u16::from_le_bytes([body[0], body[1]]);
                    let channels = u16::from_le_bytes([body[2], body[3]]);
                    let rate = u32::from_le_bytes(body[4..8].try_into().unwrap());
                    let bits = u16::from_le_bytes([body[14], body[15]]);
                    if tag == 0xFFFE && body.len() >= 26 {
                        tag = u16::from_le_bytes([body[24], body[25]]);
                    }
                    format = Some((tag, channels, rate, bits));
                    if tag == 2 {
                        adpcm = Some(AdpcmFormat::parse(body));
                    }
                },
                b"data" => data = Some(body),
                _ => (),
            }
            at += 8 + size + (size & 1);
        }
        let ((tag, channels, rate, bits), data) = (format.ok_or(WavError::MissingChunk)?, data.ok_or(WavError::MissingChunk)?);
        let channel_count = ChannelCount::new(channels).ok_or(WavError::Unsupported)?;
        let sample_rate = SampleRate::new(rate).ok_or(WavError::Unsupported)?;
        let samples: Vec<Sample> = match (tag, bits) {
            (2, 4) => decode_ms_adpcm(data, usize::from(channels), &adpcm.ok_or(WavError::Unsupported)?)
                .ok_or(WavError::Unsupported)?,
            (1, 8) => data.iter().map(|&b| (f32::from(b) - 128.0) / 128.0).collect(),
            (1, 16) => data.chunks_exact(2).map(|c| f32::from(i16::from_le_bytes([c[0], c[1]])) / 32768.0).collect(),
            (1, 24) => data
                .chunks_exact(3)
                .map(|c| (i32::from_le_bytes([0, c[0], c[1], c[2]]) >> 8) as f32 / 8_388_608.0)
                .collect(),
            (1, 32) => data
                .chunks_exact(4)
                .map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]) as f32 / 2_147_483_648.0)
                .collect(),
            (3, 32) => data.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect(),
            _ => return Err(WavError::Unsupported),
        };
        Ok(Self { samples: samples.into(), channels: channel_count, sample_rate, pos: 0 })
    }

    /// Total number of samples (all channels).
    pub fn length(&self) -> usize {
        self.samples.len()
    }
}

impl Source for WavPlayer {
    fn channel_count(&self) -> ChannelCount {
        self.channels
    }

    fn sample_rate(&self) -> SampleRate {
        self.sample_rate
    }

    fn write_samples(&mut self, buffer: &mut [Sample]) -> usize {
        let count = buffer.len().min(self.samples.len() - self.pos);
        buffer[..count].copy_from_slice(&self.samples[self.pos..self.pos + count]);
        self.pos += count;
        count
    }

    fn reset(&mut self) {
        self.pos = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ms_adpcm_blocks_decode_with_warmup_samples_and_adaptation() {
        // Mono, block_align 10: 7 header bytes + 3 data bytes (6 nibbles). Predictor 0 = (256, 0).
        let mut fmt = vec![0u8; 22];
        fmt[0] = 2; fmt[2] = 1; // tag 2, mono
        fmt[4..8].copy_from_slice(&22050u32.to_le_bytes());
        fmt[12..14].copy_from_slice(&10u16.to_le_bytes());
        fmt[14] = 4; // bits
        fmt[16] = 32; // cbSize
        fmt[18..20].copy_from_slice(&8u16.to_le_bytes()); // samples per block
        fmt[20..22].copy_from_slice(&1u16.to_le_bytes()); // one coefficient pair
        fmt.extend_from_slice(&256i16.to_le_bytes());
        fmt.extend_from_slice(&0i16.to_le_bytes());
        let mut block = vec![0u8]; // predictor index
        block.extend_from_slice(&16i16.to_le_bytes()); // delta
        block.extend_from_slice(&100i16.to_le_bytes()); // sample1
        block.extend_from_slice(&50i16.to_le_bytes()); // sample2
        block.extend_from_slice(&[0x20, 0x0F, 0x80]); // nibbles 2,0,0,15,8,0
        let mut wav = b"RIFF".to_vec();
        wav.extend_from_slice(&0u32.to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&(fmt.len() as u32).to_le_bytes());
        wav.extend_from_slice(&fmt);
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(block.len() as u32).to_le_bytes());
        wav.extend_from_slice(&block);
        let player = WavPlayer::new(&wav).expect("MS ADPCM must decode");
        assert_eq!(player.length(), 8);
        let raw: Vec<i32> = player.samples.iter().map(|s| (s * 32768.0).round() as i32).collect();
        // warm-up 50, 100; then 100+2*16=132 (delta 16*230>>8 -> 16); nibbles 0,0 keep 132; nibble 15 (= -1) gives 132-16=116
        assert_eq!(&raw[..6], &[50, 100, 132, 132, 132, 116]);
    }

    fn wav_16(channels: u16, rate: u32, samples: &[i16]) -> Vec<u8> {
        let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * u32::from(channels) * 2).to_le_bytes());
        out.extend_from_slice(&(channels * 2).to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&data);
        out
    }

    #[test]
    fn wav_decodes_and_ends() {
        let mut wav = WavPlayer::new(wav_16(1, 22_050, &[0, 16384, -16384, 32767])).unwrap();
        assert_eq!((wav.channel_count().get(), wav.sample_rate().get(), wav.length()), (1, 22_050, 4));
        let mut out = [9.0; 8];
        assert_eq!(wav.write_samples(&mut out), 4);
        assert_eq!(&out[..3], &[0.0, 0.5, -0.5]);
        assert_eq!(wav.write_samples(&mut out), 0);
        wav.reset();
        assert_eq!(wav.write_samples(&mut out[..2]), 2);
    }

    #[test]
    fn wav_rejects_garbage() {
        assert_eq!(WavPlayer::new(b"nope".as_ref()).err(), Some(WavError::NotRiffWave));
        let mut bad = wav_16(1, 8000, &[1, 2]);
        bad[20] = 99; // unknown format tag
        assert_eq!(WavPlayer::new(bad).err(), Some(WavError::Unsupported));
    }

    #[test]
    fn rechanneler_duplicates_mono_and_averages_stereo() {
        let mono = WavPlayer::new(wav_16(1, 8000, &[16384, -16384])).unwrap();
        let mut stereo = Rechanneler::new(mono, ChannelCount::new(2).unwrap());
        let mut out = [0.0; 4];
        assert_eq!(stereo.write_samples(&mut out), 4);
        assert_eq!(out, [0.5, 0.5, -0.5, -0.5]);
        let st = WavPlayer::new(wav_16(2, 8000, &[16384, 0, 0, -16384])).unwrap();
        let mut mono = Rechanneler::new(st, ChannelCount::new(1).unwrap());
        let mut out = [0.0; 2];
        assert_eq!(mono.write_samples(&mut out), 2);
        assert_eq!(out, [0.25, -0.25]);
    }

    #[test]
    fn resampler_changes_length_proportionally() {
        let samples: Vec<i16> = (0..400).map(|i| (i * 40) as i16).collect();
        let wav = WavPlayer::new(wav_16(1, 24_000, &samples)).unwrap();
        let mut resampler = Resampler::new(wav, SampleRate::new(48_000).unwrap());
        let mut out = vec![0.0; 2000];
        let written = resampler.write_samples(&mut out);
        // Doubling the rate doubles the sample count (give or take the interpolation tail).
        assert!((written as i64 - 798).abs() <= 4, "wrote {written}");
        assert!(out[1] > out[0] && out[3] >= out[2], "interpolated ramp must rise");
        assert_eq!(resampler.write_samples(&mut out), 0);
    }

    #[test]
    fn cycle_repeats_without_gaps() {
        let wav = WavPlayer::new(wav_16(1, 8000, &[1000, 2000, 3000])).unwrap();
        let mut looped = Cycle::new(wav);
        let mut out = [0.0; 8];
        assert_eq!(looped.write_samples(&mut out), 8);
        assert_eq!(out[0], out[3]);
        assert_eq!(out[1], out[4]);
        assert_eq!(out[2], out[5]);
        assert_eq!(out[6], out[0]);
    }
}
