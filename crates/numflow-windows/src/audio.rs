use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
};

use crossbeam_channel::{Sender, TrySendError};

const AUDIO_QUEUE_CAPACITY: usize = 4;
const CUE_DURATION_MS: u32 = 55;
const NUMFLOW_ON_HZ: u32 = 880;
const NUMFLOW_OFF_HZ: u32 = 520;

#[link(name = "kernel32")]
unsafe extern "system" {
    #[link_name = "Beep"]
    fn system_beep(frequency: u32, duration_ms: u32) -> i32;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioCue {
    NumFlowOn,
    NumFlowOff,
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
    running: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl AudioFeedbackService {
    /// Starts a dedicated audio worker so synchronous Win32 tone playback can never block the
    /// keyboard hook or pointer runtime.
    ///
    /// # Errors
    ///
    /// Returns [`AudioFeedbackError`] if the worker thread cannot be spawned.
    pub fn start() -> Result<Self, AudioFeedbackError> {
        let (sender, receiver) = crossbeam_channel::bounded(AUDIO_QUEUE_CAPACITY);
        let enabled = Arc::new(AtomicBool::new(true));
        let running = Arc::new(AtomicBool::new(true));
        let worker_running = Arc::clone(&running);

        let join = thread::Builder::new()
            .name("numflow-audio-feedback".to_owned())
            .spawn(move || {
                while let Ok(cue) = receiver.recv() {
                    if !worker_running.load(Ordering::Acquire) {
                        break;
                    }
                    let (frequency, duration_ms) = cue_tone(cue);
                    let _ = unsafe { system_beep(frequency, duration_ms) };
                }
            })
            .map_err(AudioFeedbackError::ThreadSpawn)?;

        Ok(Self {
            sender: Some(sender),
            enabled,
            running,
            join: Some(join),
        })
    }

    /// Enables or disables mode-switch sounds. This is intentionally exposed now so Settings can
    /// persist this preference later without changing the audio service contract.
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Release);
    }

    #[must_use]
    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }

    /// Queues a short mode cue without waiting for playback. If a user toggles faster than the
    /// bounded audio queue can drain, stale sounds are dropped rather than delaying input.
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

const fn cue_tone(cue: AudioCue) -> (u32, u32) {
    match cue {
        AudioCue::NumFlowOn => (NUMFLOW_ON_HZ, CUE_DURATION_MS),
        AudioCue::NumFlowOff => (NUMFLOW_OFF_HZ, CUE_DURATION_MS),
    }
}

#[cfg(test)]
mod tests {
    use super::{AudioCue, CUE_DURATION_MS, cue_tone};

    #[test]
    fn mode_cues_are_short_and_distinct() {
        let on = cue_tone(AudioCue::NumFlowOn);
        let off = cue_tone(AudioCue::NumFlowOff);

        assert_ne!(on.0, off.0);
        assert_eq!(on.1, CUE_DURATION_MS);
        assert_eq!(off.1, CUE_DURATION_MS);
    }
}
