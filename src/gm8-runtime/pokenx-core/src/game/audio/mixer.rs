use super::SoundParams;
use crate::audio_src::{ChannelCount, Sample, SampleRate, Source};
use gm8_audio_output::{PcmFormat, PcmProducer};
use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
    mpsc::{self, Receiver, Sender},
};

const INIT_CAPACITY: usize = 16;

/// An audio mixer designed specifically for GM8 (derived from udon's Mixer shape; no udon code).
pub struct Mixer {
    channels: ChannelCount,
    sample_rate: SampleRate,
    sources: Vec<(Box<dyn Source + Send + 'static>, Arc<SoundParams>, i32)>,
    exclusive_source: Option<(Box<dyn Source + Send + 'static>, Option<Arc<SoundParams>>, i32)>,
    global_volume: Arc<AtomicU32>,
    input_buffer: Vec<Sample>,
    receiver: Receiver<Command>,
}

enum Command {
    Add { source: Box<dyn Source + Send + 'static>, params: Arc<SoundParams>, id: i32 },
    AddExclusive { source: Box<dyn Source + Send + 'static>, params: Option<Arc<SoundParams>>, id: i32 },
    Stop(i32),
    StopAll,
}

/// Returned from Mixer::new(), and permanently associated with the Mixer created alongside it.
/// Used for dynamically adding sounds to the Mixer with `handle.add()`
pub struct MixerHandle(Sender<Command>);

impl MixerHandle {
    /// A handle that sends into a channel with no receiver, so every `add`/`stop`
    /// silently does nothing. Used where audio output is unavailable.
    pub fn new_silent() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        drop(rx);
        Self(tx)
    }
}

/// Error type for Mixer calls
#[derive(Debug, Clone, Copy)]
pub enum Error {
    /// Indicates that something could not be sent to the Mixer via a MixerHandle.
    /// This usually happens because the Mixer no longer exists.
    SendError,
}

impl Mixer {
    pub fn new(sample_rate: SampleRate, channels: ChannelCount, global_volume: Arc<AtomicU32>) -> (Self, MixerHandle) {
        let (sender, receiver) = mpsc::channel();
        (
            Self {
                channels,
                sample_rate,
                sources: Vec::with_capacity(INIT_CAPACITY),
                exclusive_source: None,
                global_volume,
                input_buffer: Vec::new(),
                receiver,
            },
            MixerHandle(sender),
        )
    }
}

impl PcmProducer for Mixer {
    fn format(&self) -> PcmFormat {
        // Udon's non-zero wrappers are constructed from validated device
        // values in AudioManager::new.
        PcmFormat::new(self.sample_rate.into(), self.channels.into()).unwrap()
    }

    fn render(&mut self, buffer: &mut [Sample]) -> usize {
        // Check for new incoming commands
        while let Ok(cmd) = self.receiver.try_recv() {
            match cmd {
                Command::Add { source, params, id } => self.sources.push((source, params, id)),
                Command::AddExclusive { source, params, id } => self.exclusive_source = Some((source, params, id)),
                Command::Stop(id) => {
                    self.sources.retain(|(_, _, x)| *x != id);
                    if let Some((_, _, x)) = &self.exclusive_source {
                        if *x == id {
                            self.exclusive_source = None;
                        }
                    }
                },
                Command::StopAll => {
                    self.sources.clear();
                    self.exclusive_source = None;
                },
            }
        }

        if let Some((source, params, _id)) = &mut self.exclusive_source {
            let count = source.write_samples(buffer);
            // Music is exclusive and used to skip the global volume.
            let local_gain = params.as_ref().map_or(1.0, |params| f32::from_bits(params.volume.load(Ordering::Acquire)));
            let music_gain = local_gain * f32::from_bits(self.global_volume.load(Ordering::Acquire));
            let end = count.min(buffer.len());
            buffer[..end].iter_mut().for_each(|x| *x *= music_gain);
            if buffer.len() != count {
                // The exclusive source ended early, so the mixer drops it (this is what stops a
                // looping music track).
                buffer[count..].iter_mut().for_each(|x| *x = 0.0);
                self.exclusive_source = None;
            }
        } else {
            buffer.iter_mut().for_each(|x| *x = 0.0);
        }

        let input_buffer = &mut self.input_buffer;
        input_buffer.resize_with(buffer.len(), Default::default);
        let global_volume = f32::from_bits(self.global_volume.load(Ordering::Acquire));

        RetainMut::retain_mut(&mut self.sources, |(source, params, _)| {
            let volume = f32::from_bits(params.volume.load(Ordering::Acquire));
            let count = source.write_samples(input_buffer);

            for (in_sample, out_sample) in input_buffer.iter().take(count).copied().zip(buffer.iter_mut()) {
                *out_sample += in_sample * volume * global_volume;
            }

            count == input_buffer.len()
        });

        // Soft knee above 0.8 instead of the hard clamp at the output stage, which crackles when sounds overlap.
        for sample in buffer.iter_mut() {
            let a = sample.abs();
            if a > 0.8 {
                *sample = sample.signum() * (0.8 + 0.2 * { let t = (a - 0.8) / 0.2; t / (1.0 + t) });
            }
        }

        buffer.len()
    }

    fn reset_output(&mut self) {
        self.sources.clear();
        self.exclusive_source = None;
    }
}

impl MixerHandle {
    /// Adds a sound to be mixed, along with its ID and atomic params
    pub fn add(&self, source: impl Source + Send + 'static, params: Arc<SoundParams>, id: i32) -> Result<(), Error> {
        let command = Command::Add { source: Box::new(source), params, id };
        self.0.send(command).map_err(|_| Error::SendError)
    }

    /// Adds an exclusive sound
    pub fn add_exclusive(&self, source: impl Source + Send + 'static, id: i32) -> Result<(), Error> {
        let command = Command::AddExclusive { source: Box::new(source), params: None, id };
        self.0.send(command).map_err(|_| Error::SendError)
    }

    /// Exclusive WAV/MIDI still obey per-sound volume and fades.
    pub fn add_exclusive_with_params(&self, source: impl Source + Send + 'static, params: Arc<SoundParams>, id: i32) -> Result<(), Error> {
        self.0.send(Command::AddExclusive { source: Box::new(source), params: Some(params), id })
            .map_err(|_| Error::SendError)
    }

    /// Stops all sounds with a certain ID
    pub fn stop(&self, id: i32) -> Result<(), Error> {
        self.0.send(Command::Stop(id)).map_err(|_| Error::SendError)
    }

    /// Stops all sounds
    pub fn stop_all(&self) -> Result<(), Error> {
        self.0.send(Command::StopAll).map_err(|_| Error::SendError)
    }
}

trait RetainMut<T> {
    fn retain_mut(&mut self, f: impl FnMut(&mut T) -> bool);
}

impl<T> RetainMut<T> for Vec<T> {
    fn retain_mut(&mut self, mut f: impl FnMut(&mut T) -> bool) {
        let len = self.len();
        let mut del = 0;
        {
            let v = &mut **self;

            for i in 0..len {
                if !f(&mut v[i]) {
                    del += 1;
                } else if del > 0 {
                    v.swap(i - del, i);
                }
            }
        }
        if del > 0 {
            self.truncate(len - del);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Tone;
    impl Source for Tone {
        fn channel_count(&self) -> ChannelCount { ChannelCount::new(2).unwrap() }
        fn sample_rate(&self) -> SampleRate { SampleRate::new(48_000).unwrap() }
        fn write_samples(&mut self, out: &mut [Sample]) -> usize { out.fill(0.5); out.len() }
        fn reset(&mut self) {}
    }

    #[test]
    fn exclusive_volume_fade_and_reset_apply_to_music() {
        let master = Arc::new(AtomicU32::new(0.5f32.to_bits()));
        let params = Arc::new(SoundParams { volume: AtomicU32::new(0.5f32.to_bits()) });
        let (mut mixer, handle) = Mixer::new(SampleRate::new(48_000).unwrap(), ChannelCount::new(2).unwrap(), master);
        handle.add_exclusive_with_params(Tone, params.clone(), 3).unwrap();
        let mut out = [0.0; 8];
        mixer.render(&mut out);
        assert_eq!(out, [0.125; 8]);
        params.volume.store(0.0f32.to_bits(), Ordering::Release);
        mixer.render(&mut out);
        assert_eq!(out, [0.0; 8]);
        params.volume.store(1.0f32.to_bits(), Ordering::Release);
        mixer.reset_output();
        mixer.render(&mut out);
        assert_eq!(out, [0.0; 8]);
    }
}
