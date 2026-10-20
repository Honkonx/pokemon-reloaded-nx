#![no_std]

//! Platform-neutral pull interface for already-mixed PCM.
//!
//! Decoding, sound state, resampling and mixing stay in the runtime. A platform
//! output backend owns the device and pulls interleaved `f32` frames through
//! [`PcmProducer`]. The count returned by `render` is a count of samples (not
//! frames); samples after that count are owned by the caller and must be treated
//! as silence by an adapter that requires fully initialized buffers.

/// The format of samples produced by the runtime mixer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PcmFormat {
    pub sample_rate_hz: u32,
    pub channels: u16,
}

impl PcmFormat {
    pub const fn new(sample_rate_hz: u32, channels: u16) -> Option<Self> {
        if sample_rate_hz == 0 || channels == 0 {
            None
        } else {
            Some(Self { sample_rate_hz, channels })
        }
    }
}

/// Pulls interleaved, normalized `f32` PCM from the game mixer.
pub trait PcmProducer {
    fn format(&self) -> PcmFormat;

    /// Fill as many samples as are available and return the number written.
    fn render(&mut self, output: &mut [f32]) -> usize;

    /// Reset backend-facing stream state. This must not recreate game sounds.
    fn reset_output(&mut self);
}

/// Convert normalized float PCM to Horizon's signed 16-bit PCM representation.
/// Non-finite values become silence; samples are clamped before scaling.
pub fn convert_f32_to_i16(input: &[f32], output: &mut [i16]) -> usize {
    let count = input.len().min(output.len());
    for (source, destination) in input[..count].iter().zip(output[..count].iter_mut()) {
        let sample = if source.is_finite() { source.clamp(-1.0, 1.0) } else { 0.0 };
        *destination = if sample <= -1.0 {
            i16::MIN
        } else {
            // `f32::round` is not in `core`; round half away from zero by hand.
            let scaled = sample * i16::MAX as f32;
            (if scaled >= 0.0 { scaled + 0.5 } else { scaled - 0.5 }) as i16
        };
    }
    count
}

/// Bounded ownership ledger for a fixed set of device buffers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BufferQueue<const N: usize> {
    queued: [bool; N],
}

impl<const N: usize> BufferQueue<N> {
    pub const fn new() -> Self { Self { queued: [false; N] } }
    pub fn next_free(&self) -> Option<usize> { self.queued.iter().position(|queued| !queued) }
    pub fn mark_queued(&mut self, index: usize) -> bool {
        match self.queued.get_mut(index) {
            Some(state) if !*state => { *state = true; true }
            _ => false,
        }
    }
    pub fn mark_released(&mut self, index: usize) -> bool {
        match self.queued.get_mut(index) {
            Some(state) if *state => { *state = false; true }
            _ => false,
        }
    }
    pub fn is_queued(&self, index: usize) -> bool { self.queued.get(index).copied().unwrap_or(false) }
}

impl<const N: usize> Default for BufferQueue<N> { fn default() -> Self { Self::new() } }

#[cfg(test)]
mod tests {
    use super::*;

    struct SyntheticTone {
        cursor: usize,
    }

    impl PcmProducer for SyntheticTone {
        fn format(&self) -> PcmFormat {
            PcmFormat::new(48_000, 2).unwrap()
        }

        fn render(&mut self, output: &mut [f32]) -> usize {
            for sample in output.iter_mut() {
                *sample = if self.cursor % 2 == 0 { 0.25 } else { -0.25 };
                self.cursor += 1;
            }
            output.len()
        }

        fn reset_output(&mut self) {
            self.cursor = 0;
        }
    }

    #[test]
    fn format_rejects_invalid_device_parameters() {
        assert_eq!(PcmFormat::new(0, 2), None);
        assert_eq!(PcmFormat::new(48_000, 0), None);
        assert_eq!(PcmFormat::new(48_000, 2), Some(PcmFormat { sample_rate_hz: 48_000, channels: 2 }));
    }

    #[test]
    fn producer_writes_deterministic_synthetic_interleaved_samples() {
        let mut tone = SyntheticTone { cursor: 0 };
        let mut samples = [0.0; 6];
        assert_eq!(tone.render(&mut samples), 6);
        assert_eq!(samples, [0.25, -0.25, 0.25, -0.25, 0.25, -0.25]);
        tone.reset_output();
        let mut next = [0.0; 2];
        assert_eq!(tone.render(&mut next), 2);
        assert_eq!(next, [0.25, -0.25]);
    }

    #[test]
    fn float_conversion_saturates_and_silences_non_finite_values() {
        let input = [-2.0, -1.0, -0.5, 0.0, 0.5, 1.0, 2.0, f32::NAN, f32::INFINITY];
        let mut output = [123; 9];
        assert_eq!(convert_f32_to_i16(&input, &mut output), 9);
        assert_eq!(output, [i16::MIN, i16::MIN, -16384, 0, 16384, i16::MAX, i16::MAX, 0, 0]);
    }

    #[test]
    fn queue_tracks_bounded_submission_and_release_ownership() {
        let mut queue = BufferQueue::<2>::new();
        assert_eq!(queue.next_free(), Some(0));
        assert!(queue.mark_queued(0));
        assert!(!queue.mark_queued(0));
        assert_eq!(queue.next_free(), Some(1));
        assert!(queue.mark_queued(1));
        assert_eq!(queue.next_free(), None);
        assert!(!queue.mark_released(2));
        assert!(queue.mark_released(0));
        assert_eq!(queue.next_free(), Some(0));
        assert!(!queue.mark_released(0));
    }
}
