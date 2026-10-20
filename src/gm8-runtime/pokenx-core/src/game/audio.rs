pub mod midi;
#[cfg(target_os = "android")]
mod aaudio;
mod mixer;
mod mp3;

use crate::audio_src::{ChannelCount, Cycle, Rechanneler, Resampler, Sample, SampleRate, Source, WavPlayer};
use gm8_audio_output::PcmProducer;
use std::{
    collections::HashMap,
    num::{NonZeroU16, NonZeroU32},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
    },
};

#[cfg(target_os = "android")]
pub fn sound_started() -> bool {
    aaudio::sound_started()
}

pub use self::midi::MidiHandle;
use self::{
    mixer::{Mixer, MixerHandle},
    mp3::Mp3Player,
};

#[derive(Clone)]
pub struct Mp3Handle {
    player: Mp3Player,
    id: i32,
}

#[derive(Clone)]
pub struct WavHandle {
    player: WavPlayer,
    params: Arc<SoundParams>,
    _use_3d: bool,
    exclusive: bool,
    id: i32,
}

pub struct SoundParams {
    pub volume: AtomicU32,
}

pub struct AudioManager {
    mixer_handle: MixerHandle,
    mixer_channel_count: ChannelCount,
    mixer_sample_rate: SampleRate,
    do_output: bool,
    global_volume: Arc<AtomicU32>,
    end_times: HashMap<i32, Option<u128>>,
    multimedia_end: Option<(i32, Option<u128>)>,
    multimedia_midi: bool,
    midi_tempo: Arc<AtomicU32>,
}

/// The mixer lives here so the host's audio thread can pull PCM while the game thread queues
/// sounds through its `MixerHandle`.
static HOST_MIXER: Mutex<Option<Mixer>> = Mutex::new(None);

/// Output format the host audio device is configured for.
pub const OUTPUT_SAMPLE_RATE: u32 = 48_000;
pub const OUTPUT_CHANNELS: u16 = 2;

/// Fills `output` (interleaved `f32` at `OUTPUT_SAMPLE_RATE`, `OUTPUT_CHANNELS`) with mixed game
/// audio; silence when no game is running or the mixer is busy. Called by the host audio thread.
pub fn render_host_audio(output: &mut [Sample]) -> usize {
    match HOST_MIXER.try_lock() {
        Ok(mut guard) => match guard.as_mut() {
            Some(mixer) => mixer.render(output),
            None => {
                output.fill(0.0);
                output.len()
            },
        },
        Err(_) => {
            output.fill(0.0);
            output.len()
        },
    }
}

impl AudioManager {
    pub fn new(do_output: bool, _capture_audio: bool) -> Self {
        let sample_rate = NonZeroU32::new(OUTPUT_SAMPLE_RATE).unwrap();
        let channel_count = NonZeroU16::new(OUTPUT_CHANNELS).unwrap();
        let global_volume = Arc::new(AtomicU32::from(1.0f32.to_bits()));
        let (mixer, mixer_handle) = Mixer::new(sample_rate, channel_count, global_volume.clone());
        *HOST_MIXER.lock().unwrap() = Some(mixer);

        Self {
            mixer_handle,
            mixer_channel_count: channel_count,
            mixer_sample_rate: sample_rate,
            do_output,
            global_volume,
            end_times: HashMap::new(),
            multimedia_end: None,
            multimedia_midi: false,
            midi_tempo: Arc::new(AtomicU32::new(1.0f32.to_bits())),
        }
    }

    pub fn add_mp3(&mut self, file: Box<[u8]>, sound_id: i32) -> Option<Mp3Handle> {
        Mp3Player::new(file).map(|player| Mp3Handle { player, id: sound_id }).ok()
    }
    pub fn add_wav(
        &mut self,
        file: Box<[u8]>,
        sound_id: i32,
        volume: f64,
        use_3d: bool,
        exclusive: bool,
    ) -> Option<WavHandle> {
        WavPlayer::new(file)
            .map(|player| WavHandle {
                player,
                params: Arc::new(SoundParams { volume: AtomicU32::new(make_volume(volume).to_bits()) }),
                _use_3d: use_3d,
                exclusive,
                id: sound_id,
            })
            .ok()
    }

    /// Loads a MIDI file. Nothing is synthesized here: the song is rendered block by block while
    /// it plays, so changing the music costs a parse rather than a full render.
    pub fn add_midi(&mut self, data: Box<[u8]>, sound_id: i32, volume: f64, exclusive: bool) -> Option<MidiHandle> {
        MidiHandle::new(data, sound_id, volume, exclusive)
    }

    pub fn play_mp3(&mut self, handle: &Mp3Handle, start_time: u128) {
        self.multimedia_midi = false;
        let end_time = length_to_ns(
            handle.player.length(),
            handle.player.sample_rate().into(),
            1, // mp3 length() already takes channels into account
        ) + start_time;
        self.multimedia_end = Some((handle.id, Some(end_time)));
        if self.do_output {
            let _ = self.mixer_handle.add_exclusive(
                Rechanneler::new(
                    Resampler::new(handle.player.clone(), self.mixer_sample_rate),
                    self.mixer_channel_count,
                ),
                handle.id,
            );
        }
    }

    pub fn play_wav(&mut self, handle: &WavHandle, start_time: u128) {
        let end_time = length_to_ns(
            handle.player.length(),
            handle.player.sample_rate().into(),
            handle.player.channel_count().into(),
        ) + start_time;
        if handle.exclusive {
            self.multimedia_midi = false;
            self.multimedia_end = Some((handle.id, Some(end_time)));
        } else if self.end_times.get(&handle.id) != Some(&None) {
            self.end_times.insert(handle.id, Some(end_time));
        }

        if self.do_output {
            if handle.exclusive {
                let _ = self.mixer_handle.add_exclusive_with_params(
                    Rechanneler::new(
                        Resampler::new(handle.player.clone(), self.mixer_sample_rate),
                        self.mixer_channel_count,
                    ),
                    handle.params.clone(),
                    handle.id,
                );
            } else {
                let _ = self.mixer_handle.add(
                    Rechanneler::new(
                        Resampler::new(handle.player.clone(), self.mixer_sample_rate),
                        self.mixer_channel_count,
                    ),
                    handle.params.clone(),
                    handle.id,
                );
            }
        }
    }

    pub fn play_midi(&mut self, handle: &MidiHandle, start_time: u128) {
        let duration = length_to_ns(handle.length(), handle.sample_rate(), handle.channel_count());
        let tempo = if handle.exclusive { f32::from_bits(self.midi_tempo.load(Ordering::Acquire)) as f64 } else { 1.0 };
        let end_time = start_time.saturating_add((duration as f64 / tempo) as u128);
        if handle.exclusive {
            self.multimedia_midi = true;
            self.multimedia_end = Some((handle.id, Some(end_time)));
        } else if self.end_times.get(&handle.id) != Some(&None) {
            self.end_times.insert(handle.id, Some(end_time));
        }

        if self.do_output {
            let source = if handle.exclusive { handle.source_with_tempo(false, self.midi_tempo.clone()) } else { handle.source(false) };
            if let Some(source) = source {
                if handle.exclusive {
                    let _ = self.mixer_handle.add_exclusive_with_params(
                        Rechanneler::new(
                            Resampler::new(source, self.mixer_sample_rate),
                            self.mixer_channel_count,
                        ),
                        handle.params.clone(),
                        handle.id,
                    );
                } else {
                    let _ = self.mixer_handle.add(
                        Rechanneler::new(
                            Resampler::new(source, self.mixer_sample_rate),
                            self.mixer_channel_count,
                        ),
                        handle.params.clone(),
                        handle.id,
                    );
                }
            }
        }
    }

    pub fn loop_midi(&mut self, handle: &MidiHandle) {
        if handle.exclusive {
            self.multimedia_midi = true;
            self.multimedia_end = Some((handle.id, None));
        } else {
            self.end_times.insert(handle.id, None);
        }

        if self.do_output {
            // The sequencer loops the file itself, so this source never ends and there is no need
            // to cycle it at the mixer.
            let source = if handle.exclusive { handle.source_with_tempo(true, self.midi_tempo.clone()) } else { handle.source(true) };
            if let Some(source) = source {
                if handle.exclusive {
                    let _ = self.mixer_handle.add_exclusive_with_params(
                        Rechanneler::new(
                            Resampler::new(source, self.mixer_sample_rate),
                            self.mixer_channel_count,
                        ),
                        handle.params.clone(),
                        handle.id,
                    );
                } else {
                    let _ = self.mixer_handle.add(
                        Rechanneler::new(
                            Resampler::new(source, self.mixer_sample_rate),
                            self.mixer_channel_count,
                        ),
                        handle.params.clone(),
                        handle.id,
                    );
                }
            }
        }
    }

    pub fn loop_mp3(&mut self, handle: &Mp3Handle) {
        self.multimedia_midi = false;
        self.multimedia_end = Some((handle.id, None));
        eprintln!("[mixer] loop_mp3 id={} do_output={}", handle.id, self.do_output);
        if self.do_output {
            let res = self.mixer_handle.add_exclusive(
                Cycle::new(Rechanneler::new(
                    Resampler::new(handle.player.clone(), self.mixer_sample_rate),
                    self.mixer_channel_count,
                )),
                handle.id,
            );
            eprintln!("[mixer] add_exclusive mp3 loop id={} -> {:?}", handle.id, res);
        }
    }

    pub fn loop_wav(&mut self, handle: &WavHandle) {
        if handle.exclusive {
            self.multimedia_midi = false;
            self.multimedia_end = Some((handle.id, None));
        } else {
            self.end_times.insert(handle.id, None);
        }

        if self.do_output {
            if handle.exclusive {
                let res = self.mixer_handle.add_exclusive_with_params(
                    Cycle::new(Rechanneler::new(
                        Resampler::new(handle.player.clone(), self.mixer_sample_rate),
                        self.mixer_channel_count,
                    )),
                    handle.params.clone(),
                    handle.id,
                );
                eprintln!("[mixer] add_exclusive loop id={} -> {:?}", handle.id, res);
            } else {
                let res = self.mixer_handle.add(
                    Cycle::new(Rechanneler::new(
                        Resampler::new(handle.player.clone(), self.mixer_sample_rate),
                        self.mixer_channel_count,
                    )),
                    handle.params.clone(),
                    handle.id,
                );
                eprintln!("[mixer] add loop id={} -> {:?}", handle.id, res);
            }
        }
    }

    pub fn stop_sound(&mut self, id: i32) {
        self.end_times.remove(&id);
        if self.multimedia_end.map(|(x, _)| x) == Some(id) {
            self.multimedia_end = None;
            self.multimedia_midi = false;
        }
        if self.do_output {
            let _ = self.mixer_handle.stop(id);
        }
    }

    pub fn stop_all(&mut self) {
        self.end_times.clear();
        self.multimedia_end = None;
        self.multimedia_midi = false;
        if self.do_output {
            let _ = self.mixer_handle.stop_all();
        }
    }

    pub fn set_global_volume(&self, vol: f64) {
        self.global_volume.store(make_volume(vol).to_bits(), Ordering::Release)
    }

    /// Change MIDI event timing without resampling the synthesizer/pitch.
    /// Keep the legacy logical sound_isplaying deadline in the same timebase.
    pub fn set_background_tempo(&mut self, factor: f64, now: u128) -> bool {
        if !factor.is_finite() { return false; }
        let factor = factor.clamp(0.01, 100.0) as f32;
        let previous = f32::from_bits(self.midi_tempo.load(Ordering::Acquire));
        if self.multimedia_midi {
            if let Some((_, Some(end))) = &mut self.multimedia_end {
                let remaining = end.saturating_sub(now);
                *end = now.saturating_add((remaining as f64 * f64::from(previous) / f64::from(factor)) as u128);
            }
        }
        self.midi_tempo.store(factor.to_bits(), Ordering::Release);
        true
    }

    pub fn sound_playing(&self, sound_id: i32, current_time: u128) -> bool {
        self.mp3_playing(sound_id, current_time) || self.wav_playing(sound_id, current_time)
    }

    fn mp3_playing(&self, sound_id: i32, current_time: u128) -> bool {
        self.multimedia_end
            .map(|(id, end_time)| id == sound_id && end_time.map(|x| x > current_time).unwrap_or(true))
            .unwrap_or(false)
    }

    fn wav_playing(&self, sound_id: i32, current_time: u128) -> bool {
        match self.end_times.get(&sound_id) {
            Some(&Some(x)) => x > current_time,
            Some(None) => true,
            None => false,
        }
    }

    pub fn state(&self) -> AudioState {
        AudioState {
            global_volume: self.global_volume.clone(),
            end_times: self.end_times.clone(),
            multimedia_end: self.multimedia_end,
            multimedia_midi: self.multimedia_midi,
            midi_tempo: f32::from_bits(self.midi_tempo.load(Ordering::Acquire)),
        }
    }

    pub fn set_state(&mut self, state: AudioState) {
        self.global_volume = state.global_volume;
        self.end_times = state.end_times;
        self.multimedia_end = state.multimedia_end;
        self.multimedia_midi = state.multimedia_midi;
        self.midi_tempo.store(state.midi_tempo.to_bits(), Ordering::Release);
    }
}

impl WavHandle {
    pub fn set_volume(&self, vol: f64) {
        self.params.volume.store(make_volume(vol).to_bits(), Ordering::Release);
    }
}

#[derive(Clone)]
pub struct AudioState {
    global_volume: Arc<AtomicU32>,
    end_times: HashMap<i32, Option<u128>>,
    multimedia_end: Option<(i32, Option<u128>)>,
    multimedia_midi: bool,
    midi_tempo: f32,
}

fn length_to_ns(sample_count: usize, sample_rate: u32, channels: u16) -> u128 {
    (sample_count as u128 * 1_000_000_000) / (u128::from(sample_rate) * u128::from(channels))
}

#[cfg(test)]
mod tempo_tests {
    use super::*;

    #[test]
    fn background_tempo_updates_midi_deadline_state_and_leaves_mp3_unchanged() {
        let mut bytes = b"MThd\0\0\0\x06\0\0\0\x01\x01\xe0MTrk\0\0\0\x0d".to_vec();
        bytes.extend_from_slice(&[0, 0x90, 60, 100, 0x83, 0x60, 0x80, 60, 0, 0, 0xff, 0x2f, 0]);
        let mut audio = AudioManager::new(false, false);
        let midi = audio.add_midi(bytes.into_boxed_slice(), 7, 1.0, true).unwrap();
        audio.play_midi(&midi, 0);
        assert!(audio.sound_playing(7, 499_000_000));
        assert!(!audio.sound_playing(7, 501_000_000));
        assert!(audio.set_background_tempo(2.0, 250_000_000));
        assert!(audio.sound_playing(7, 374_000_000));
        assert!(!audio.sound_playing(7, 376_000_000));
        let saved = audio.state();
        assert!(audio.set_background_tempo(0.5, 250_000_000));
        assert!(audio.sound_playing(7, 749_000_000));
        audio.set_state(saved);
        assert!(!audio.sound_playing(7, 376_000_000));
        assert!(!audio.set_background_tempo(f64::NAN, 0));
        assert!(!audio.set_background_tempo(f64::INFINITY, 0));
        assert_eq!(f32::from_bits(audio.midi_tempo.load(Ordering::Acquire)), 2.0);
        audio.loop_midi(&midi);
        audio.set_background_tempo(1000.0, 0);
        assert_eq!(f32::from_bits(audio.midi_tempo.load(Ordering::Acquire)), 100.0);
        assert!(audio.sound_playing(7, u128::MAX));
        audio.set_background_tempo(-1.0, 0);
        assert_eq!(f32::from_bits(audio.midi_tempo.load(Ordering::Acquire)), 0.01f32);
        audio.stop_sound(7);
        assert!(!audio.sound_playing(7, 0));
        let mp3 = audio.add_mp3(crate::title_tests::fixture::mp3_wave_with_metadata().into_boxed_slice(), 8).unwrap();
        audio.play_mp3(&mp3, 0);
        let end = audio.multimedia_end;
        audio.set_background_tempo(2.0, 0);
        assert_eq!(audio.multimedia_end, end, "tempo is MIDI-only, not MP3 pitch/speed");
    }
}

/// Some of the game's `.wav` files are really MP3 streams carrying a WAVE header (format tag
/// 0x0055, MPEG Layer 3), which the WAV decoder rejects with UnknownFormat. They have to go to
/// the MP3 decoder instead.
pub(crate) fn wav_is_mp3(data: &[u8]) -> bool {
    mp3_wave_payload(data).is_some()
}

/// Finds only the encoded data bytes, excluding RIFF metadata and padding.
/// Feeding the pad byte to the MPEG decoder can make its last frame look
/// truncated, and metadata may itself contain false MPEG sync patterns.
fn mp3_wave_payload(data: &[u8]) -> Option<core::ops::Range<usize>> {
    if data.len() < 12 || &data[..4] != b"RIFF" || &data[8..12] != b"WAVE" {
        return None;
    }
    let riff_size = u32::from_le_bytes(data[4..8].try_into().unwrap()) as usize;
    let end = riff_size.checked_add(8).filter(|&end| end >= 12 && end <= data.len())?;
    let mut offset = 12usize;
    let mut format = None;
    let mut payload = None;
    while offset < end {
        let start = offset.checked_add(8).filter(|&start| start <= end)?;
        let size = u32::from_le_bytes(data[offset + 4..start].try_into().unwrap()) as usize;
        let chunk_end = start.checked_add(size).filter(|&n| n <= end)?;
        match &data[offset..offset + 4] {
            b"fmt " => {
                if size < 16 || format.is_some() { return None; }
                format = Some(data[start..start + 2] == [0x55, 0]);
            }
            b"data" => {
                if payload.is_some() { return None; }
                payload = Some(start..chunk_end);
            }
            _ => {}
        }
        offset = chunk_end.checked_add(size & 1).filter(|&n| n <= end)?;
    }
    if format == Some(true) { payload } else { None }
}

/// Decodes a WAV/MP3 exactly like playback would, but without needing an audio device. Used by
/// the `--scan-assets` diagnostic to catch sounds the game would fail to load.
pub(crate) fn check_wav(file: Box<[u8]>) -> Result<(), String> {
    if wav_is_mp3(&file) {
        return check_mp3(file)
    }
    WavPlayer::new(file).map(|_| ()).map_err(|e| format!("{:?}", e))
}

pub(crate) fn check_mp3(file: Box<[u8]>) -> Result<(), String> {
    Mp3Player::new(file).map(|_| ()).map_err(|_| "no pude decodificar el MP3".to_string())
}

/// Stops and resumes the audio output when the app goes to the background and comes back.
pub fn set_output_paused(paused: bool) {
    #[cfg(target_os = "android")]
    aaudio::set_paused(paused);
    #[cfg(not(target_os = "android"))]
    let _ = paused;
}

// This function takes a volume between 0.0 and 1.0 and converts it to the logarithmic scale used by DirectMusic.
// This is, roughly, the same function used by GM8/DirectMusic.
// Note that the minimum possible output from this function is 0.001. I think that's accurate to GM8.
pub(crate) fn make_volume(vol: f64) -> f32 {
    1000.0f64.powf(vol.clamp(0.0, 1.0) - 1.0) as f32
}
