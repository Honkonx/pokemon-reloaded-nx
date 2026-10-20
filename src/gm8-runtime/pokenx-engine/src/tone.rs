//! Synthetic PCM used to validate the audio path with no game data.

pub const SAMPLE_RATE: u32 = 48_000;
const FREQUENCY_HZ: u32 = 440;
const AMPLITUDE: i16 = 6_000;

/// Fills interleaved stereo S16 frames with a square wave while `active`, silence otherwise.
/// `phase` counts samples and is kept by the caller across calls so the wave is continuous.
pub fn fill_stereo(out: &mut [i16], phase: &mut u32, active: bool) {
    let half_period = SAMPLE_RATE / FREQUENCY_HZ / 2;
    for frame in out.chunks_exact_mut(2) {
        let sample = if active && (*phase / half_period) % 2 == 0 { AMPLITUDE } else if active { -AMPLITUDE } else { 0 };
        frame[0] = sample;
        frame[1] = sample;
        *phase = phase.wrapping_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silent_when_inactive() {
        let mut buffer = [1i16; 64];
        let mut phase = 0;
        fill_stereo(&mut buffer, &mut phase, false);
        assert!(buffer.iter().all(|&s| s == 0));
        assert_eq!(phase, 32);
    }

    #[test]
    fn square_wave_alternates_and_is_continuous() {
        let mut first = [0i16; 2 * 200];
        let mut second = [0i16; 2 * 200];
        let mut phase = 0;
        fill_stereo(&mut first, &mut phase, true);
        fill_stereo(&mut second, &mut phase, true);
        assert!(first.iter().any(|&s| s == AMPLITUDE) && first.iter().any(|&s| s == -AMPLITUDE));
        // Left and right channels carry the same sample.
        assert!(first.chunks_exact(2).all(|f| f[0] == f[1]));
        // Continuity: one long fill equals the two consecutive fills.
        let mut whole = [0i16; 2 * 400];
        let mut phase = 0;
        fill_stereo(&mut whole, &mut phase, true);
        assert_eq!(&whole[..400], &first[..]);
        assert_eq!(&whole[400..], &second[..]);
    }
}
