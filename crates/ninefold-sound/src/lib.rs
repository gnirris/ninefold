//! Sound, out of the player's own copy of the game.
//!
//! Two pieces: a decoder that turns one of 1.8's ogg files into samples, and a mixer that adds
//! however many are playing together and hands the result to the sound card. Both are small on
//! purpose — a game this old has no streaming music to worry about and no sound that is longer
//! than a second or two, so every clip is decoded whole and kept.
//!
//! Everything is mixed down to mono on the way in and panned on the way out. A 1.8 ogg is mono
//! anyway, but a few of the records are not, and a stereo clip positioned in the world would
//! fight with its own panning.

use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// How many sounds may play at once. Past this the oldest is dropped, which is what vanilla does
/// and is better than the clipping that comes of adding forty things together.
const MAX_VOICES: usize = 24;

/// A decoded sound, ready to play as often as you like.
#[derive(Clone)]
pub struct Clip {
    samples: Arc<[f32]>,
    rate: u32,
}

impl Clip {
    /// How long it lasts.
    #[must_use]
    pub fn seconds(&self) -> f32 {
        self.samples.len() as f32 / self.rate.max(1) as f32
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }
}

/// Decode an ogg Vorbis file into mono samples.
#[must_use]
pub fn decode(bytes: &[u8]) -> Option<Clip> {
    let mut reader =
        lewton::inside_ogg::OggStreamReader::new(std::io::Cursor::new(bytes.to_vec())).ok()?;
    let channels = usize::from(reader.ident_hdr.audio_channels).max(1);
    let rate = reader.ident_hdr.audio_sample_rate;
    let mut samples: Vec<f32> = Vec::new();
    // read_dec_packet_itl gives interleaved i16, which is the cheapest thing to fold to mono.
    while let Ok(Some(packet)) = reader.read_dec_packet_itl() {
        for frame in packet.chunks(channels) {
            let total: f32 = frame.iter().map(|s| f32::from(*s) / 32768.0).sum();
            samples.push(total / channels as f32);
        }
    }
    if samples.is_empty() {
        return None;
    }
    Some(Clip {
        samples: samples.into(),
        rate,
    })
}

/// Add every playing voice together into one buffer of interleaved frames.
///
/// This runs on the sound card's own thread, under a deadline: it must not allocate, must not
/// block for long, and must leave the buffer fully written whatever happens.
fn render(voices: &Mutex<Vec<Voice>>, out: &mut [f32], channels: usize) {
    out.fill(0.0);
    let Ok(mut voices) = voices.lock() else {
        // A poisoned lock means a panic in whoever held it. Silence beats taking this thread down.
        return;
    };
    for frame in out.chunks_mut(channels) {
        let (mut left, mut right) = (0.0f32, 0.0f32);
        for voice in voices.iter_mut() {
            if let Some((l, r)) = voice.next() {
                left += l;
                right += r;
            }
        }
        // Soft clip: several sounds together can leave the range, and a hard clamp on a loud
        // moment sounds like a fault rather than like loudness.
        let shape = |v: f32| {
            let v = v.clamp(-1.5, 1.5);
            (v - v * v * v / 3.0).clamp(-1.0, 1.0)
        };
        let (left, right) = (shape(left), shape(right));
        for (channel, slot) in frame.iter_mut().enumerate() {
            *slot = if channel % 2 == 0 { left } else { right };
        }
    }
    voices.retain(|v| (v.at as usize) < v.samples.len());
}

/// One sound in the middle of playing.
struct Voice {
    samples: Arc<[f32]>,
    /// Where in the clip we are, in samples. Fractional, because pitch resamples.
    at: f64,
    /// How far to advance per output frame: the clip's rate against the card's, times pitch.
    step: f64,
    left: f32,
    right: f32,
}

impl Voice {
    /// The next sample, interpolated between the two it falls between.
    fn next(&mut self) -> Option<(f32, f32)> {
        let index = self.at as usize;
        let first = *self.samples.get(index)?;
        let second = *self.samples.get(index + 1).unwrap_or(&first);
        let fraction = (self.at - index as f64) as f32;
        let value = first + (second - first) * fraction;
        self.at += self.step;
        Some((value * self.left, value * self.right))
    }
}

/// An open sound card with whatever is playing on it.
pub struct Mixer {
    voices: Arc<Mutex<Vec<Voice>>>,
    rate: u32,
    /// Held because dropping it stops the sound.
    _stream: cpal::Stream,
}

impl Mixer {
    /// Open the default output. `None` if there is nothing to play through, which is not an
    /// error worth stopping a game for.
    #[must_use]
    pub fn open() -> Option<Self> {
        let host = cpal::default_host();
        let device = host.default_output_device()?;
        let supported = device.default_output_config().ok()?;
        let format = supported.sample_format();
        let config = supported.config();
        let channels = usize::from(config.channels).max(1);
        let rate = config.sample_rate;

        let voices: Arc<Mutex<Vec<Voice>>> = Arc::new(Mutex::new(Vec::new()));
        let error = |e| eprintln!("sound: {e}");
        let stream = match format {
            cpal::SampleFormat::F32 => {
                let playing = voices.clone();
                device
                    .build_output_stream(
                        config,
                        move |data: &mut [f32], _| render(&playing, data, channels),
                        error,
                        None,
                    )
                    .ok()?
            }
            cpal::SampleFormat::I16 => {
                let playing = voices.clone();
                device
                    .build_output_stream(
                        config,
                        move |data: &mut [i16], _| {
                            let mut mixed = vec![0.0f32; data.len()];
                            render(&playing, &mut mixed, channels);
                            for (slot, value) in data.iter_mut().zip(mixed) {
                                *slot = (value * 32767.0) as i16;
                            }
                        },
                        error,
                        None,
                    )
                    .ok()?
            }
            // Something exotic. Better no sound than a stream full of noise.
            _ => return None,
        };
        stream.play().ok()?;
        Some(Self {
            voices,
            rate,
            _stream: stream,
        })
    }

    /// Start a clip. `pan` is -1 hard left to 1 hard right.
    pub fn play(&self, clip: &Clip, volume: f32, pitch: f32, pan: f32) {
        if clip.is_empty() || volume <= 0.0 {
            return;
        }
        let Ok(mut voices) = self.voices.lock() else {
            return;
        };
        if voices.len() >= MAX_VOICES {
            voices.remove(0);
        }
        let pan = pan.clamp(-1.0, 1.0);
        voices.push(Voice {
            samples: clip.samples.clone(),
            at: 0.0,
            step: f64::from(clip.rate) / f64::from(self.rate.max(1)) * f64::from(pitch.max(0.05)),
            // Equal-power panning, so moving a sound across does not make it louder in the middle.
            left: volume * ((1.0 - pan) * 0.5).sqrt(),
            right: volume * ((1.0 + pan) * 0.5).sqrt(),
        });
    }

    /// How many sounds are playing right now.
    #[must_use]
    pub fn playing(&self) -> usize {
        self.voices.lock().map(|v| v.len()).unwrap_or(0)
    }

    /// The card's sample rate.
    #[must_use]
    pub const fn rate(&self) -> u32 {
        self.rate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rubbish_is_not_a_sound() {
        assert!(decode(b"this is not an ogg file").is_none());
        assert!(decode(&[]).is_none());
    }

    #[test]
    fn a_voice_walks_through_its_samples_and_stops() {
        let samples: Arc<[f32]> = vec![1.0, 1.0, 1.0].into();
        let mut voice = Voice {
            samples: samples.clone(),
            at: 0.0,
            step: 1.0,
            left: 0.5,
            right: 0.5,
        };
        for _ in 0..3 {
            let (l, r) = voice.next().expect("still playing");
            assert!((l - 0.5).abs() < 1e-6 && (r - 0.5).abs() < 1e-6);
        }
        assert!(voice.next().is_none(), "and then it is finished");
    }

    #[test]
    fn pitch_resamples_rather_than_skipping() {
        // Half speed through a ramp: the value between two samples is the average of them.
        let samples: Arc<[f32]> = vec![0.0, 1.0].into();
        let mut voice = Voice {
            samples,
            at: 0.0,
            step: 0.5,
            left: 1.0,
            right: 1.0,
        };
        assert_eq!(voice.next().map(|(l, _)| l), Some(0.0));
        assert_eq!(voice.next().map(|(l, _)| l), Some(0.5));
    }

    #[test]
    fn panning_keeps_the_power_the_same_across_the_middle() {
        let power = |pan: f32| {
            let l = ((1.0 - pan) * 0.5f32).sqrt();
            let r = ((1.0 + pan) * 0.5f32).sqrt();
            l * l + r * r
        };
        for pan in [-1.0, -0.5, 0.0, 0.5, 1.0] {
            assert!((power(pan) - 1.0).abs() < 1e-5, "pan {pan}");
        }
    }
}
