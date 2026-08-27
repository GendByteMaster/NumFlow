use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    thread::{self, JoinHandle},
};

use crossbeam_channel::{Sender, TrySendError};
use numflow_core::{CoreEffect, PointerEffect, StateChange};
use windows::{
    Win32::Media::Audio::{PlaySoundA, SND_MEMORY, SND_NODEFAULT},
    core::PCSTR,
};

const AUDIO_QUEUE_CAPACITY: usize = 6;

const TOGGLE_ON_WAV: &[u8] = include_bytes!("../../../assets/sfx/glass/toggle-on.wav");
const TOGGLE_OFF_WAV: &[u8] = include_bytes!("../../../assets/sfx/glass/toggle-off.wav");
const SELECT_WAV: &[u8] = include_bytes!("../../../assets/sfx/glass/select.wav");
const OPEN_WAV: &[u8] = include_bytes!("../../../assets/sfx/glass/open.wav");
const CLOSE_WAV: &[u8] = include_bytes!("../../../assets/sfx/glass/close.wav");
const EXPAND_WAV: &[u8] = include_bytes!("../../../assets/sfx/glass/expand.wav");
const COLLAPSE_WAV: &[u8] = include_bytes!("../../../assets/sfx/glass/collapse.wav");
const DRAG_START_WAV: &[u8] = include_bytes!("../../../assets/sfx/glass/drag-start.wav");
const RELEASE_WAV: &[u8] = include_bytes!("../../../assets/sfx/glass/release.wav");
const DELETE_WAV: &[u8] = include_bytes!("../../../assets/sfx/glass/delete.wav");
const ERROR_WAV: &[u8] = include_bytes!("../../../assets/sfx/glass/error.wav");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioCue {
    NumFlowOn,
    NumFlowOff,
    Select,
    ToggleOn,
    ToggleOff,
    Open,
    Close,
    Expand,
    Collapse,
    DragStart,
    Release,
    Delete,
    Error,
}

#[derive(Debug, thiserror::Error)]
pub enum AudioFeedbackError {
    #[error("failed to spawn the NumFlow audio feedback thread: {0}")]
    ThreadSpawn(#[source] io::Error),
}

#[derive(Debug)]
pub struct AudioFeedbackService {
    sender: Option<Sender<AudioCue>>,
    enabled: Arc<AtomicBool>,
    volume_percent: Arc<AtomicU8>,
    running: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl AudioFeedbackService {
    /// Starts a dedicated worker so audio feedback never blocks the keyboard hook or pointer loop.
    ///
    /// # Errors
    ///
    /// Returns [`AudioFeedbackError`] if the worker thread cannot be spawned.
    pub fn start() -> Result<Self, AudioFeedbackError> {
        let (sender, receiver) = crossbeam_channel::bounded(AUDIO_QUEUE_CAPACITY);
        let enabled = Arc::new(AtomicBool::new(true));
        let volume_percent = Arc::new(AtomicU8::new(25));
        let running = Arc::new(AtomicBool::new(true));
        let worker_running = Arc::clone(&running);
        let worker_volume_percent = Arc::clone(&volume_percent);

        let join = thread::Builder::new()
            .name("numflow-audio-feedback".to_owned())
            .spawn(move || {
                while let Ok(cue) = receiver.recv() {
                    if !worker_running.load(Ordering::Acquire) {
                        break;
                    }
                    play_wave(cue, worker_volume_percent.load(Ordering::Acquire));
                }
            })
            .map_err(AudioFeedbackError::ThreadSpawn)?;

        Ok(Self {
            sender: Some(sender),
            enabled,
            volume_percent,
            running,
            join: Some(join),
        })
    }

    /// Enables or disables all semantic UI feedback handled by this service.
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Release);
    }

    #[must_use]
    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }

    /// Sets semantic feedback volume without touching the Windows mixer.
    pub fn set_volume_percent(&self, volume_percent: u8) {
        self.volume_percent
            .store(volume_percent.min(100), Ordering::Release);
    }

    /// Queues one short semantic cue. Stale cues are dropped instead of delaying input.
    pub fn play(&self, cue: AudioCue) {
        if !self.enabled() {
            return;
        }
        let Some(sender) = &self.sender else {
            return;
        };
        match sender.try_send(cue) {
            Ok(()) | Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {}
        }
    }

    /// Chooses at most one meaningful cue for a batch of core effects.
    /// Pointer movement and ordinary clicks intentionally stay silent to avoid noisy feedback.
    pub fn play_effects(&self, effects: &[CoreEffect]) {
        if let Some(cue) = cue_for_effects(effects) {
            self.play(cue);
        }
    }
}

impl Drop for AudioFeedbackService {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Release);
        self.sender.take();
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn play_wave(cue: AudioCue, volume_percent: u8) {
    let volume_percent = volume_percent.min(100);
    if volume_percent == 0 {
        return;
    }

    let mut bytes = wave_bytes(cue).to_vec();
    attenuate_pcm16_wave(&mut bytes, i32::from(volume_percent));

    // Playback is synchronous only on this dedicated audio worker. Keeping the temporary buffer
    // alive until PlaySound returns lets NumFlow apply runtime attenuation safely without touching
    // the user's global Windows mixer volume or the keyboard/pointer threads.
    let _ = unsafe { PlaySoundA(PCSTR(bytes.as_ptr()), None, SND_MEMORY | SND_NODEFAULT) };
}

fn attenuate_pcm16_wave(bytes: &mut [u8], gain_percent: i32) {
    let gain_percent = gain_percent.clamp(0, 100);
    if gain_percent == 100 {
        return;
    }

    let Some((data_start, data_end)) = wave_data_range(bytes) else {
        return;
    };

    let (samples, _) = bytes[data_start..data_end].as_chunks_mut::<2>();
    for sample in samples {
        let value = i32::from(i16::from_le_bytes([sample[0], sample[1]]));
        let scaled = value * gain_percent / 100;
        let encoded = i16::try_from(scaled)
            .expect("attenuated 16-bit PCM sample must remain in range")
            .to_le_bytes();
        sample.copy_from_slice(&encoded);
    }
}

fn wave_data_range(bytes: &[u8]) -> Option<(usize, usize)> {
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }

    let mut offset = 12usize;
    while offset.checked_add(8)? <= bytes.len() {
        let chunk_size = u32::from_le_bytes([
            bytes[offset + 4],
            bytes[offset + 5],
            bytes[offset + 6],
            bytes[offset + 7],
        ]);
        let chunk_size = usize::try_from(chunk_size).ok()?;
        let data_start = offset.checked_add(8)?;
        let data_end = data_start.checked_add(chunk_size)?.min(bytes.len());

        if &bytes[offset..offset + 4] == b"data" {
            return Some((data_start, data_end));
        }

        let padded_size = chunk_size.checked_add(chunk_size & 1)?;
        offset = data_start.checked_add(padded_size)?;
    }

    None
}

fn wave_bytes(cue: AudioCue) -> &'static [u8] {
    match cue {
        AudioCue::NumFlowOn | AudioCue::ToggleOn => TOGGLE_ON_WAV,
        AudioCue::NumFlowOff | AudioCue::ToggleOff => TOGGLE_OFF_WAV,
        AudioCue::Select => SELECT_WAV,
        AudioCue::Open => OPEN_WAV,
        AudioCue::Close => CLOSE_WAV,
        AudioCue::Expand => EXPAND_WAV,
        AudioCue::Collapse => COLLAPSE_WAV,
        AudioCue::DragStart => DRAG_START_WAV,
        AudioCue::Release => RELEASE_WAV,
        AudioCue::Delete => DELETE_WAV,
        AudioCue::Error => ERROR_WAV,
    }
}

fn cue_for_effects(effects: &[CoreEffect]) -> Option<AudioCue> {
    let mut state_cue = None;
    for effect in effects {
        match effect {
            CoreEffect::Pointer(PointerEffect::ButtonDown(_)) => return Some(AudioCue::DragStart),
            CoreEffect::Pointer(PointerEffect::ButtonUp(_)) => return Some(AudioCue::Release),
            CoreEffect::State(StateChange::Precision(enabled)) => {
                state_cue = Some(if *enabled {
                    AudioCue::ToggleOn
                } else {
                    AudioCue::ToggleOff
                });
            }
            CoreEffect::State(StateChange::SelectedButton(_)) if state_cue.is_none() => {
                state_cue = Some(AudioCue::Select);
            }
            CoreEffect::Pointer(PointerEffect::Move(_) | PointerEffect::Click { .. })
            | CoreEffect::State(StateChange::Enabled(_) | StateChange::SelectedButton(_)) => {}
        }
    }
    state_cue
}

#[cfg(test)]
mod tests {
    use numflow_core::{CoreEffect, MouseButton, PointerEffect, StateChange};

    use super::{AudioCue, attenuate_pcm16_wave, cue_for_effects, wave_bytes};

    #[test]
    fn semantic_effects_map_to_non_noisy_cues() {
        assert_eq!(
            cue_for_effects(&[CoreEffect::State(StateChange::SelectedButton(
                MouseButton::Right,
            ))]),
            Some(AudioCue::Select)
        );
        assert_eq!(
            cue_for_effects(&[CoreEffect::State(StateChange::Precision(true))]),
            Some(AudioCue::ToggleOn)
        );
        assert_eq!(
            cue_for_effects(&[CoreEffect::Pointer(PointerEffect::ButtonDown(
                MouseButton::Left,
            ))]),
            Some(AudioCue::DragStart)
        );
        assert_eq!(
            cue_for_effects(&[CoreEffect::Pointer(PointerEffect::ButtonUp(
                MouseButton::Left,
            ))]),
            Some(AudioCue::Release)
        );
        assert_eq!(
            cue_for_effects(&[CoreEffect::Pointer(PointerEffect::Move(
                numflow_core::Direction::Right,
            ))]),
            None
        );
    }

    #[test]
    fn embedded_ui_sfx_are_valid_wave_images() {
        for cue in [
            AudioCue::NumFlowOn,
            AudioCue::NumFlowOff,
            AudioCue::Select,
            AudioCue::Open,
            AudioCue::Close,
            AudioCue::Expand,
            AudioCue::Collapse,
            AudioCue::DragStart,
            AudioCue::Release,
            AudioCue::Delete,
            AudioCue::Error,
        ] {
            let bytes = wave_bytes(cue);
            assert!(bytes.starts_with(b"RIFF"));
            assert_eq!(&bytes[8..12], b"WAVE");
        }
    }

    #[test]
    fn runtime_attenuation_reduces_pcm_samples() {
        let mut wave = b"RIFF\x28\0\0\0WAVEfmt \x10\0\0\0\x01\0\x01\0\x44\xac\0\0\x88\x58\x01\0\x02\0\x10\0data\x04\0\0\0\x10\x27\xf0\xd8".to_vec();

        attenuate_pcm16_wave(&mut wave, 50);

        let data = &wave[44..48];
        assert_eq!(i16::from_le_bytes([data[0], data[1]]), 5_000);
        assert_eq!(i16::from_le_bytes([data[2], data[3]]), -5_000);
    }
}
