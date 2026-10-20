// GameMaker 8 stores "BackgroundMusic" and "Multimedia" sounds as MIDI on Windows, where they
// were played through the system media player. Since they're commonly used for background music,
// they are synthesized to PCM here, which lets them go through the same mixer as everything else.
//
// The whole song used to be rendered up front, the moment the game changed the music. A song is
// minutes long, so that froze the game for seconds every time the player crossed into another
// zone or a battle started. Nothing is rendered ahead of time any more: [MidiSource] synthesizes
// one mixer block at a time, as playback advances, which is a fraction of a millisecond per block.

use crate::{
    audio_src::{ChannelCount, Sample, SampleRate, Source},
    rustysynth::{MidiFile, MidiFileSequencer, SoundFont, Synthesizer, SynthesizerSettings},
};
use std::{
    io::Cursor,
    path::PathBuf,
    sync::{atomic::AtomicU32, Arc},
};

use super::{make_volume, SoundParams};

const SAMPLE_RATE: u32 = 44100;
const CHANNELS: u16 = 2;
/// Scratch space for one mixer block, so the first callback after a song change does not have to
/// grow the buffers.
const INITIAL_BLOCK: usize = 4096;

/// The GM SoundFont is not part of the program: the user puts a `.sf2` here (the Android port
/// bundles TimGM6mb, a General MIDI font). Without it MIDI music is silent.
pub const SOUNDFONT_PATH: &str = "sdmc:/switch/pokenx/soundfont.sf2";

fn soundfont_candidates() -> Vec<PathBuf> {
    vec![PathBuf::from(SOUNDFONT_PATH)]
}

fn load_soundfont() -> Option<Vec<u8>> {
    for path in soundfont_candidates() {
        match std::fs::read(&path) {
            Ok(bytes) => return Some(bytes),
            Err(e) => eprintln!("could not load soundfont '{}': {}", path.display(), e),
        }
    }
    eprintln!("no SoundFont (.sf2) found for MIDI playback: copy one to {}", SOUNDFONT_PATH);
    None
}

/// The parsed SoundFont, shared by every synthesizer.
///
/// Parsing it costs real time (the file is several megabytes) and it was being parsed again for
/// every single song, on top of the synthesis. It never changes, so it is parsed once.
fn shared_soundfont() -> Option<Arc<SoundFont>> {
    static SOUNDFONT: std::sync::OnceLock<Option<Arc<SoundFont>>> = std::sync::OnceLock::new();
    SOUNDFONT
        .get_or_init(|| {
            let bytes = load_soundfont()?;
            Some(Arc::new(SoundFont::new(&mut Cursor::new(&bytes)).ok()?))
        })
        .clone()
}

/// Parses a MIDI file. A few of the game's files are RIFF/RMID (a bare MIDI wrapped in a RIFF
/// container), which the parser rejects, so skip to the embedded MThd chunk when there is one.
fn parse(data: &[u8]) -> Result<MidiFile, crate::rustysynth::MidiFileError> {
    let data = match data.windows(4).position(|window| window == b"MThd") {
        Some(offset) => &data[offset..],
        None => data,
    };
    MidiFile::new(&mut Cursor::new(data))
}

/// How long a parsed file lasts, in interleaved samples - the unit the sound handles measure
/// themselves in.
fn length_in_samples(midi: &MidiFile) -> usize {
    // The vendored `get_length` returns 0 for a file with no timed events instead of panicking.
    let seconds = midi.get_length();
    let frames = (seconds.max(0.0) * f64::from(SAMPLE_RATE)) as usize;
    frames * usize::from(CHANNELS)
}

/// Validates a MIDI file without rendering it. Rendering takes seconds per song, which is far too
/// slow for the `--scan-assets` diagnostic.
pub(crate) fn check(data: &[u8]) -> Result<(), String> {
    parse(data).map(|_| ()).map_err(|e| format!("{:?}", e))
}

/// A MIDI file that is ready to play, and that can be played more than once.
///
/// The parsed file is deliberately not kept: the raw bytes are what get cloned and serialized
/// (savestates carry them like any other sound), and parsing again costs a fraction of a
/// millisecond, while keeping live synthesis state around cannot be serialized at all.
#[derive(Clone)]
pub struct MidiHandle {
    data: Box<[u8]>,
    pub(super) params: Arc<SoundParams>,
    pub(super) id: i32,
    pub(super) exclusive: bool,
    length: usize,
}

impl MidiHandle {
    /// Loads a MIDI file, checking that it can be played and measuring how long it lasts.
    pub fn new(data: Box<[u8]>, sound_id: i32, volume: f64, exclusive: bool) -> Option<Self> {
        let length = length_in_samples(&parse(&data).ok()?);
        Some(Self {
            data,
            params: Arc::new(SoundParams { volume: AtomicU32::new(make_volume(volume).to_bits()) }),
            id: sound_id,
            exclusive,
            length,
        })
    }

    pub fn sample_rate(&self) -> u32 {
        SAMPLE_RATE
    }

    pub fn channel_count(&self) -> u16 {
        CHANNELS
    }

    /// The length of the file in interleaved samples, used to work out when playback ends.
    pub fn length(&self) -> usize {
        self.length
    }

    pub fn set_volume(&self, vol: f64) {
        self.params.volume.store(make_volume(vol).to_bits(), std::sync::atomic::Ordering::Release);
    }

    /// A fresh playback of this file. Every play gets its own sequencer, so the same sound can be
    /// played again while an earlier copy is still going.
    pub fn source(&self, looping: bool) -> Option<MidiSource> {
        MidiSource::new(&self.data, looping)
    }

    pub(super) fn source_with_tempo(&self, looping: bool, tempo: Arc<AtomicU32>) -> Option<MidiSource> {
        let mut source = self.source(looping)?;
        source.tempo = tempo;
        Some(source)
    }
}

/// Synthesizes a MIDI file block by block, as the mixer pulls samples.
pub struct MidiSource {
    tempo: Arc<AtomicU32>,
    sequencer: MidiFileSequencer,
    midi: Arc<MidiFile>,
    /// The file was started with looping on, so it never reaches the end.
    looping: bool,
    /// The sequence finished and the last block has already been handed over; the next call
    /// reports the end so the mixer drops this source.
    finished: bool,
    left: Vec<f32>,
    right: Vec<f32>,
}

impl MidiSource {
    fn new(data: &[u8], looping: bool) -> Option<Self> {
        Self::with_soundfont(data, looping, shared_soundfont()?)
    }

    fn with_soundfont(data: &[u8], looping: bool, soundfont: Arc<SoundFont>) -> Option<Self> {
        let settings = SynthesizerSettings::new(i32::try_from(SAMPLE_RATE).ok()?);
        let synthesizer = Synthesizer::new(&soundfont, &settings).ok()?;
        let midi = Arc::new(parse(data).ok()?);

        let mut sequencer = MidiFileSequencer::new(synthesizer);
        sequencer.play(&midi, looping);

        Some(Self {
            tempo: Arc::new(AtomicU32::new(1.0f32.to_bits())),
            sequencer,
            midi,
            looping,
            finished: false,
            left: vec![0.0; INITIAL_BLOCK],
            right: vec![0.0; INITIAL_BLOCK],
        })
    }
}

impl Source for MidiSource {
    fn channel_count(&self) -> ChannelCount {
        ChannelCount::new(CHANNELS).unwrap()
    }

    fn sample_rate(&self) -> SampleRate {
        SampleRate::new(SAMPLE_RATE).unwrap()
    }

    fn write_samples(&mut self, buffer: &mut [Sample]) -> usize {
        let channels = usize::from(CHANNELS);
        let frames = buffer.len() / channels;
        if frames == 0 || self.finished {
            return 0
        }

        self.left.resize(frames, 0.0);
        self.right.resize(frames, 0.0);
        self.left.fill(0.0);
        self.right.fill(0.0);
        self.sequencer.set_speed(f32::from_bits(self.tempo.load(std::sync::atomic::Ordering::Acquire)) as f64);
        self.sequencer.render(&mut self.left, &mut self.right);

        for (frame, (left, right)) in self.left.iter().zip(self.right.iter()).enumerate() {
            buffer[frame * channels] = *left;
            buffer[frame * channels + 1] = *right;
        }
        // The mixer asks for whole frames, but never leave anything it did not ask for unwritten.
        for sample in &mut buffer[frames * channels..] {
            *sample = 0.0;
        }

        if !self.looping && self.sequencer.end_of_sequence() {
            // This block still holds the tail of the song, so it is handed over and the end is
            // reported on the following call, once it has been heard.
            self.finished = true;
        }

        buffer.len()
    }

    fn reset(&mut self) {
        self.sequencer.play(&self.midi, self.looping);
        self.finished = false;
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;
    use crate::audio_src::{Rechanneler, Resampler};
    use crate::game::audio::{mixer::Mixer, OUTPUT_CHANNELS, OUTPUT_SAMPLE_RATE};
    use gm8_audio_output::PcmProducer;
    use std::sync::atomic::Ordering;

    fn midi(track: &[u8]) -> Vec<u8> {
        let mut bytes = b"MThd".to_vec();
        bytes.extend_from_slice(&6u32.to_be_bytes());
        bytes.extend_from_slice(&[0, 0, 0, 1, 1, 0xE0]); // type 0, one track, 480 PPQN
        bytes.extend_from_slice(b"MTrk");
        bytes.extend_from_slice(&(track.len() as u32).to_be_bytes());
        bytes.extend_from_slice(track);
        bytes
    }

    #[test]
    #[ignore = "Requires POKENX_TEST_SOUNDFONT pointing to an attributed free SF2; no audio device"]
    fn soundfont_pcm_resampling_loop_volume_and_stop() {
        let path = std::env::var_os("POKENX_TEST_SOUNDFONT").expect("set the test SoundFont path");
        let bytes = std::fs::read(path).unwrap();
        let font = Arc::new(SoundFont::new(&mut Cursor::new(bytes)).unwrap());
        let notes = midi(&[0, 0x90, 60, 100, 0x83, 0x60, 0x80, 60, 0, 0, 0xFF, 0x2F, 0]);
        let mut source = MidiSource::with_soundfont(&notes, false, font.clone()).unwrap();
        let mut block = [0.0f32; 2048];
        let mut heard = false;
        let mut ended = false;
        for _ in 0..100 {
            let count = source.write_samples(&mut block);
            assert!(block[..count].iter().all(|sample| sample.is_finite()));
            heard |= block[..count].iter().any(|sample| sample.abs() > 0.0001);
            if count == 0 { ended = true; break }
        }
        assert!(heard && ended, "finite MIDI must produce PCM and end");
        source.reset();
        assert_eq!(source.write_samples(&mut block), block.len());
        let position = source.sequencer.get_position();
        source.tempo.store(2.0f32.to_bits(), Ordering::Release);
        assert_eq!(source.write_samples(&mut block), block.len());
        let fast_delta = source.sequencer.get_position() - position;
        assert!((0.04..0.05).contains(&fast_delta), "2x advances event time, preserving PCM sample rate");
        let position = source.sequencer.get_position();
        source.tempo.store(0.5f32.to_bits(), Ordering::Release);
        assert_eq!(source.write_samples(&mut block), block.len());
        let slow_delta = source.sequencer.get_position() - position;
        assert!((0.01..0.014).contains(&slow_delta));
        assert_eq!(source.sample_rate().get(), SAMPLE_RATE);

        let gain = Arc::new(AtomicU32::new(1.0f32.to_bits()));
        let (mut mixer, handle) = Mixer::new(SampleRate::new(OUTPUT_SAMPLE_RATE).unwrap(),
                                           ChannelCount::new(OUTPUT_CHANNELS).unwrap(), gain.clone());
        let source = MidiSource::with_soundfont(&notes, true, font.clone()).unwrap();
        let params = Arc::new(SoundParams { volume: AtomicU32::new(1.0f32.to_bits()) });
        handle.add_exclusive_with_params(Rechanneler::new(Resampler::new(source, SampleRate::new(48_000).unwrap()),
                                             ChannelCount::new(2).unwrap()), params.clone(), 7).unwrap();
        let mut heard_after_end = false;
        for index in 0..100 {
            assert_eq!(mixer.render(&mut block), block.len());
            assert!(block.iter().all(|sample| sample.is_finite() && sample.abs() <= 1.0));
            if index > 40 { heard_after_end |= block.iter().any(|sample| sample.abs() > 0.0001); }
        }
        assert!(heard_after_end, "looping MIDI must continue beyond the first song duration");
        params.volume.store(0.0f32.to_bits(), Ordering::Release);
        mixer.render(&mut block);
        assert!(block.iter().all(|sample| *sample == 0.0), "per-sound MIDI fade must reach silence");
        params.volume.store(1.0f32.to_bits(), Ordering::Release);
        gain.store(0.0f32.to_bits(), Ordering::Release);
        mixer.render(&mut block);
        assert!(block.iter().all(|sample| *sample == 0.0));
        gain.store(1.0f32.to_bits(), Ordering::Release);
        handle.stop(7).unwrap();
        mixer.render(&mut block);
        assert!(block.iter().all(|sample| *sample == 0.0));

        let mut empty = b"MThd".to_vec();
        empty.extend_from_slice(&6u32.to_be_bytes());
        empty.extend_from_slice(&[0, 0, 0, 0, 1, 0xE0]); // no tracks
        // A malformed empty file must never reach a looping sequencer.
        assert!(MidiSource::with_soundfont(&empty, true, font.clone()).is_none());

        // Exercise the vendored sequencer's defensive path independently of
        // parser rejection, and a legal marker pair with zero elapsed time.
        let settings = SynthesizerSettings::new(44100);
        let mut sequence = MidiFileSequencer::new(Synthesizer::new(&font, &settings).unwrap());
        sequence.play(&Arc::new(MidiFile { messages: vec![], times: vec![] }), true);
        sequence.render(&mut [0.0; 64], &mut [0.0; 64]);
        let markers = midi(&[0, 0xB0, 116, 0, 0, 0xB0, 117, 0, 0, 0xFF, 0x2F, 0]);
        let file = MidiFile::new_with_loop_type(&mut Cursor::new(markers), crate::rustysynth::MidiFileLoopType::FinalFantasy).unwrap();
        sequence.play(&Arc::new(file), true);
        sequence.render(&mut [0.0; 64], &mut [0.0; 64]);
    }

    #[test]
    fn malformed_midi_timing_and_header_return_errors() {
        let mut empty = midi(&[0, 0xFF, 0x2F, 0]);
        empty[10..12].copy_from_slice(&0u16.to_be_bytes());
        assert!(check(&empty).is_err());
        let mut bad_resolution = midi(&[0, 0xFF, 0x2F, 0]);
        bad_resolution[12..14].copy_from_slice(&0u16.to_be_bytes());
        assert!(check(&bad_resolution).is_err());
        assert!(check(&midi(&[0, 0xFF, 0x51, 3, 0, 0, 0, 0, 0xFF, 0x2F, 0])).is_err());
        let mut overflow = Vec::new();
        for _ in 0..9 { overflow.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0x7F, 0x90, 60, 1]); }
        overflow.extend_from_slice(&[0, 0xFF, 0x2F, 0]);
        assert!(check(&midi(&overflow)).is_err());
    }
}
