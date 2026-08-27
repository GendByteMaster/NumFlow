from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    file = Path(path)
    text = file.read_text(encoding="utf-8")
    count = text.count(old)
    if count != 1:
        raise RuntimeError(f"expected one match in {path}, found {count}: {old[:100]!r}")
    file.write_text(text.replace(old, new, 1), encoding="utf-8")


# windows crate: enable WinMM audio projection and replace synthetic Beep tones with UI SFX WAV.
replace_once(
    "crates/numflow-windows/Cargo.toml",
    '    "Win32_Graphics_Gdi",\n',
    '    "Win32_Graphics_Gdi",\n    "Win32_Media_Audio",\n',
)

Path("crates/numflow-windows/src/audio.rs").write_text(
    r'''use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
};

use crossbeam_channel::{Sender, TrySendError};
use numflow_core::{CoreEffect, PointerEffect, StateChange};
use windows::{
    Win32::Media::Audio::{PlaySoundA, SND_ASYNC, SND_MEMORY, SND_NODEFAULT},
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
        let running = Arc::new(AtomicBool::new(true));
        let worker_running = Arc::clone(&running);

        let join = thread::Builder::new()
            .name("numflow-audio-feedback".to_owned())
            .spawn(move || {
                while let Ok(cue) = receiver.recv() {
                    if !worker_running.load(Ordering::Acquire) {
                        break;
                    }
                    play_wave(cue);
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

    /// Enables or disables all semantic UI feedback handled by this service.
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Release);
    }

    #[must_use]
    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
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

fn play_wave(cue: AudioCue) {
    let bytes = wave_bytes(cue);
    // UI SFX buffers are compiled into the binary, so the memory remains valid for the complete
    // asynchronous playback required by SND_MEMORY | SND_ASYNC.
    let _ = unsafe {
        PlaySoundA(
            PCSTR(bytes.as_ptr()),
            None,
            SND_ASYNC | SND_MEMORY | SND_NODEFAULT,
        )
    };
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

    use super::{AudioCue, cue_for_effects, wave_bytes};

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
}
''',
    encoding="utf-8",
)

# Config: persisted, backward-compatible sound preference.
replace_once(
    "src/config.rs",
    "#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]\npub struct AppConfig {\n",
    "const fn default_sounds_enabled() -> bool {\n    true\n}\n\n#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]\npub struct AppConfig {\n",
)
replace_once(
    "src/config.rs",
    "    pub hud_enabled: bool,\n    pub start_minimized: bool,\n",
    "    pub hud_enabled: bool,\n    #[serde(default = \"default_sounds_enabled\")]\n    pub sounds_enabled: bool,\n    pub start_minimized: bool,\n",
)
replace_once(
    "src/config.rs",
    "            hud_enabled: true,\n            start_minimized: false,\n",
    "            hud_enabled: true,\n            sounds_enabled: true,\n            start_minimized: false,\n",
)
replace_once(
    "src/config.rs",
    "    #[test]\n    fn corrupted_config_recovers_to_safe_defaults() {\n",
    '''    #[test]
    fn legacy_config_without_sound_preference_defaults_to_enabled() {
        let serialized = toml::to_string_pretty(&AppConfig::default())
            .expect("default config should serialize");
        let legacy = serialized
            .lines()
            .filter(|line| !line.starts_with("sounds_enabled ="))
            .collect::<Vec<_>>()
            .join("\\n");
        let parsed: AppConfig = toml::from_str(&legacy).expect("legacy config should deserialize");

        assert!(parsed.sounds_enabled);
    }

    #[test]
    fn corrupted_config_recovers_to_safe_defaults() {
''',
)

# Runtime: one audio service is authoritative for both global input and UI-only cues.
replace_once(
    "src/runtime.rs",
    "pub struct RuntimeConfig {\n    pub motion: MotionConfig,\n    pub bindings: Bindings,\n    pub selected_button: MouseButton,\n    pub precision: bool,\n}\n",
    "pub struct RuntimeConfig {\n    pub motion: MotionConfig,\n    pub bindings: Bindings,\n    pub selected_button: MouseButton,\n    pub precision: bool,\n    pub sounds_enabled: bool,\n}\n",
)
replace_once(
    "src/runtime.rs",
    "            selected_button,\n            precision,\n        }\n    }\n}\n\n#[derive(Debug, Clone, Copy, PartialEq, Eq)]\npub struct RuntimeStateSnapshot",
    '''            selected_button,
            precision,
            sounds_enabled: true,
        }
    }

    #[must_use]
    pub const fn with_sounds_enabled(mut self, enabled: bool) -> Self {
        self.sounds_enabled = enabled;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiSoundCue {
    Open,
    Close,
    Expand,
    Collapse,
    ToggleOn,
    ToggleOff,
    Select,
    Delete,
    Error,
}

impl UiSoundCue {
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "open" => Some(Self::Open),
            "close" => Some(Self::Close),
            "expand" => Some(Self::Expand),
            "collapse" => Some(Self::Collapse),
            "toggle-on" => Some(Self::ToggleOn),
            "toggle-off" => Some(Self::ToggleOff),
            "select" => Some(Self::Select),
            "delete" => Some(Self::Delete),
            "error" => Some(Self::Error),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeStateSnapshot''',
)
replace_once(
    "src/runtime.rs",
    "    use super::{RuntimeConfig, RuntimeError, RuntimeEvent, RuntimeStateSnapshot};\n",
    "    use super::{RuntimeConfig, RuntimeError, RuntimeEvent, RuntimeStateSnapshot, UiSoundCue};\n",
)
replace_once(
    "src/runtime.rs",
    "        SetMotionConfig(numflow_core::MotionConfig),\n        SetBindings(numflow_core::Bindings),\n        Shutdown,\n",
    "        SetMotionConfig(numflow_core::MotionConfig),\n        SetBindings(numflow_core::Bindings),\n        SetSoundsEnabled(bool),\n        PlaySound(UiSoundCue),\n        Shutdown,\n",
)
replace_once(
    "src/runtime.rs",
    "        pub fn set_bindings(&self, bindings: numflow_core::Bindings) -> Result<(), RuntimeError> {\n            self.send(RuntimeCommand::SetBindings(bindings))\n        }\n\n        #[must_use]\n        pub fn drain_events",
    '''        pub fn set_bindings(&self, bindings: numflow_core::Bindings) -> Result<(), RuntimeError> {
            self.send(RuntimeCommand::SetBindings(bindings))
        }

        pub fn set_sounds_enabled(&self, enabled: bool) -> Result<(), RuntimeError> {
            self.send(RuntimeCommand::SetSoundsEnabled(enabled))
        }

        pub fn play_sound(&self, cue: UiSoundCue) -> Result<(), RuntimeError> {
            self.send(RuntimeCommand::PlaySound(cue))
        }

        #[must_use]
        pub fn drain_events''',
)
replace_once(
    "src/runtime.rs",
    "        let audio_feedback = match AudioFeedbackService::start() {\n            Ok(service) => Some(service),\n",
    "        let audio_feedback = match AudioFeedbackService::start() {\n            Ok(service) => {\n                service.set_enabled(config.sounds_enabled);\n                Some(service)\n            }\n",
)
# Both command select branches need access to the audio service.
text = Path("src/runtime.rs").read_text(encoding="utf-8")
old_call = """                            &mut normalizer,
                            event_sink,
                        );"""
if text.count(old_call) != 2:
    raise RuntimeError(f"expected two handle_command_message call sites, found {text.count(old_call)}")
text = text.replace(
    old_call,
    """                            &mut normalizer,
                            event_sink,
                            audio_feedback.as_ref(),
                        );""",
)
Path("src/runtime.rs").write_text(text, encoding="utf-8")
replace_once(
    "src/runtime.rs",
    "        normalizer: &mut KeyboardEventNormalizer,\n        event_sink: &RuntimeEventSink,\n    ) -> bool {\n",
    "        normalizer: &mut KeyboardEventNormalizer,\n        event_sink: &RuntimeEventSink,\n        audio_feedback: Option<&AudioFeedbackService>,\n    ) -> bool {\n",
)
replace_once(
    "src/runtime.rs",
    "                if let Err(error) = apply_command(command, machine, hook, normalizer) {\n",
    "                if let Err(error) =\n                    apply_command(command, machine, hook, normalizer, audio_feedback)\n                {\n",
)
replace_once(
    "src/runtime.rs",
    "                if !effects.is_empty() {\n                    event_sink.send(RuntimeEvent::Effects {\n                        state: machine.snapshot(),\n                        effects,\n                    });\n                }\n            }\n            Err(error) => {\n                fail_safe(machine, hook, normalizer, event_sink, &error.to_string());\n            }\n        }\n        true\n    }\n\n    fn apply_command(\n",
    '''                if !effects.is_empty() {
                    if let Some(audio_feedback) = audio_feedback {
                        audio_feedback.play_effects(&effects);
                    }
                    event_sink.send(RuntimeEvent::Effects {
                        state: machine.snapshot(),
                        effects,
                    });
                }
            }
            Err(error) => {
                fail_safe(machine, hook, normalizer, event_sink, &error.to_string());
            }
        }
        true
    }

    fn apply_command(
''',
)
replace_once(
    "src/runtime.rs",
    "        hook: &KeyboardHook,\n        normalizer: &mut KeyboardEventNormalizer,\n    ) -> Result<(), String> {\n",
    "        hook: &KeyboardHook,\n        normalizer: &mut KeyboardEventNormalizer,\n        audio_feedback: Option<&AudioFeedbackService>,\n    ) -> Result<(), String> {\n",
)
replace_once(
    "src/runtime.rs",
    "            RuntimeCommand::Apply(action) => {\n                if matches!(\n                    action,\n                    InputAction::ToggleEnabled | InputAction::SetEnabled(_)\n                ) {\n                    return Ok(());\n                }\n                machine\n                    .apply_action(action)\n                    .map_err(|error| error.to_string())?;\n",
    '''            RuntimeCommand::Apply(action) => {
                if matches!(
                    action,
                    InputAction::ToggleEnabled | InputAction::SetEnabled(_)
                ) {
                    return Ok(());
                }
                let effects = machine
                    .apply_action(action)
                    .map_err(|error| error.to_string())?;
                if let Some(audio_feedback) = audio_feedback {
                    audio_feedback.play_effects(&effects);
                }
''',
)
replace_once(
    "src/runtime.rs",
    "            RuntimeCommand::Configure(config) => {\n                machine\n                    .configure(config)\n",
    "            RuntimeCommand::Configure(config) => {\n                if let Some(audio_feedback) = audio_feedback {\n                    audio_feedback.set_enabled(config.sounds_enabled);\n                }\n                machine\n                    .configure(config)\n",
)
replace_once(
    "src/runtime.rs",
    "            RuntimeCommand::SetBindings(bindings) => {\n                machine.set_bindings(bindings);\n                normalizer.reset();\n            }\n            RuntimeCommand::Shutdown =>",
    '''            RuntimeCommand::SetBindings(bindings) => {
                machine.set_bindings(bindings);
                normalizer.reset();
            }
            RuntimeCommand::SetSoundsEnabled(enabled) => {
                if let Some(audio_feedback) = audio_feedback {
                    audio_feedback.set_enabled(enabled);
                }
            }
            RuntimeCommand::PlaySound(cue) => {
                if let Some(audio_feedback) = audio_feedback {
                    audio_feedback.play(match cue {
                        UiSoundCue::Open => AudioCue::Open,
                        UiSoundCue::Close => AudioCue::Close,
                        UiSoundCue::Expand => AudioCue::Expand,
                        UiSoundCue::Collapse => AudioCue::Collapse,
                        UiSoundCue::ToggleOn => AudioCue::ToggleOn,
                        UiSoundCue::ToggleOff => AudioCue::ToggleOff,
                        UiSoundCue::Select => AudioCue::Select,
                        UiSoundCue::Delete => AudioCue::Delete,
                        UiSoundCue::Error => AudioCue::Error,
                    });
                }
            }
            RuntimeCommand::Shutdown =>''',
)
replace_once(
    "src/runtime.rs",
    "    pub fn set_bindings(&self, _bindings: Bindings) -> Result<(), RuntimeError> {\n        Ok(())\n    }\n\n    #[must_use]\n    pub fn drain_events",
    '''    pub fn set_bindings(&self, _bindings: Bindings) -> Result<(), RuntimeError> {
        Ok(())
    }

    pub fn set_sounds_enabled(&self, _enabled: bool) -> Result<(), RuntimeError> {
        Ok(())
    }

    pub fn play_sound(&self, _cue: UiSoundCue) -> Result<(), RuntimeError> {
        Ok(())
    }

    #[must_use]
    pub fn drain_events''',
)

# App settings and UI bridge.
replace_once(
    "src/app.rs",
    "    runtime::{BackgroundRuntime, RuntimeConfig, RuntimeEvent, RuntimeStateSnapshot},\n",
    "    runtime::{BackgroundRuntime, RuntimeConfig, RuntimeEvent, RuntimeStateSnapshot, UiSoundCue},\n",
)
replace_once(
    "src/app.rs",
    "    fn hud_enabled(&self) -> bool {\n        self.config.hud_enabled\n    }\n\n    fn set_start_minimized",
    '''    fn hud_enabled(&self) -> bool {
        self.config.hud_enabled
    }

    fn set_sounds_enabled(&mut self, enabled: bool) {
        self.config.sounds_enabled = enabled;
    }

    fn sounds_enabled(&self) -> bool {
        self.config.sounds_enabled
    }

    fn set_start_minimized''',
)
replace_once(
    "src/app.rs",
    "            self.controller.selected_button(),\n            self.controller.is_precision_enabled(),\n        )\n",
    "            self.controller.selected_button(),\n            self.controller.is_precision_enabled(),\n        )\n        .with_sounds_enabled(self.sounds_enabled())\n",
)
replace_once(
    "src/app.rs",
    "    window.set_hud_enabled(settings.hud_enabled());\n    window.set_active_profile",
    "    window.set_hud_enabled(settings.hud_enabled());\n    window.set_sounds_enabled(settings.sounds_enabled());\n    window.set_active_profile",
)
replace_once(
    "src/app.rs",
    "fn runtime_set_bindings(runtime: &SharedRuntime, settings: &SharedUiSettings) {\n    let bindings = settings.borrow().bindings.clone();\n    if let Err(error) = runtime.borrow().set_bindings(bindings) {\n        tracing::error!(%error, \"failed to update background NumPad bindings\");\n    }\n}\n\n#[cfg(windows)]",
    '''fn runtime_set_bindings(runtime: &SharedRuntime, settings: &SharedUiSettings) {
    let bindings = settings.borrow().bindings.clone();
    if let Err(error) = runtime.borrow().set_bindings(bindings) {
        tracing::error!(%error, "failed to update background NumPad bindings");
    }
}

fn runtime_play_sound(runtime: &SharedRuntime, cue: UiSoundCue) {
    if let Err(error) = runtime.borrow().play_sound(cue) {
        tracing::debug!(%error, ?cue, "UI sound cue could not be queued");
    }
}

fn runtime_set_sounds_enabled(runtime: &SharedRuntime, enabled: bool) {
    if let Err(error) = runtime.borrow().set_sounds_enabled(enabled) {
        tracing::warn!(%error, enabled, "failed to update interface sound preference in runtime");
    }
}

#[cfg(windows)]''',
)
# Add sound preference and generic UI sound bridge at the beginning of connect_preferences.
replace_once(
    "src/app.rs",
    "fn connect_preferences(\n    window: &AppWindow,\n    tray: &AppTray,\n    settings: &SharedUiSettings,\n    hud: &SharedHud,\n    store: &SharedConfigStore,\n    runtime: &SharedRuntime,\n) {\n    {\n        let settings = Rc::clone(settings);\n        let hud = Rc::clone(hud);\n",
    '''fn connect_preferences(
    window: &AppWindow,
    tray: &AppTray,
    settings: &SharedUiSettings,
    hud: &SharedHud,
    store: &SharedConfigStore,
    runtime: &SharedRuntime,
) {
    {
        let settings = Rc::clone(settings);
        let store = Rc::clone(store);
        let runtime = Rc::clone(runtime);
        window.on_sounds_toggled(move |enabled| {
            if enabled {
                runtime_set_sounds_enabled(&runtime, true);
                runtime_play_sound(&runtime, UiSoundCue::ToggleOn);
            } else {
                // FIFO command ordering lets the off cue play before muting the worker.
                runtime_play_sound(&runtime, UiSoundCue::ToggleOff);
                runtime_set_sounds_enabled(&runtime, false);
            }
            settings.borrow_mut().set_sounds_enabled(enabled);
            persist_configuration(&settings, &store);
        });
    }

    {
        let settings = Rc::clone(settings);
        let runtime = Rc::clone(runtime);
        window.on_ui_sound_requested(move |name| {
            if !settings.borrow().sounds_enabled() {
                return;
            }
            if let Some(cue) = UiSoundCue::from_name(name.as_str()) {
                runtime_play_sound(&runtime, cue);
            }
        });
    }

    {
        let settings = Rc::clone(settings);
        let hud = Rc::clone(hud);
''',
)
# Runtime faults should get an error cue whenever the service remains reachable.
replace_once(
    "src/app.rs",
    "                    tracing::error!(%reason, \"NumFlow background pointer runtime entered safe disabled state\");\n                    hud.borrow_mut().observe_effects",
    "                    tracing::error!(%reason, \"NumFlow background pointer runtime entered safe disabled state\");\n                    runtime_play_sound(&runtime, UiSoundCue::Error);\n                    hud.borrow_mut().observe_effects",
)
replace_once(
    "src/app.rs",
    "        start_minimized = settings.borrow().start_minimized(),\n        start_with_windows = settings.borrow().start_with_windows(),\n",
    "        start_minimized = settings.borrow().start_minimized(),\n        start_with_windows = settings.borrow().start_with_windows(),\n        sounds_enabled = settings.borrow().sounds_enabled(),\n",
)

# Slint app: expose sound preference and semantic structural cues.
replace_once(
    "ui/app.slint",
    "    in-out property <bool> hud-enabled: true;\n    in-out property <string> active-profile: \"Normal\";\n",
    "    in-out property <bool> hud-enabled: true;\n    in-out property <bool> sounds-enabled: true;\n    in-out property <string> active-profile: \"Normal\";\n",
)
replace_once(
    "ui/app.slint",
    "    callback hud-toggled(bool);\n    callback profile-changed(string);\n",
    "    callback hud-toggled(bool);\n    callback sounds-toggled(bool);\n    callback ui-sound-requested(string);\n    callback profile-changed(string);\n",
)
replace_once(
    "ui/app.slint",
    "    preferred-height: 420px;\n    min-width: 540px;\n    min-height: 390px;\n",
    "    preferred-height: 440px;\n    min-width: 540px;\n    min-height: 410px;\n",
)
# Profile selection is a semantic selection; core profile configure itself intentionally stays quiet.
replace_once(
    "ui/app.slint",
    "                            root.profile-changed(root.active-profile);\n                            root.mark-saved();\n",
    "                            root.profile-changed(root.active-profile);\n                            root.ui-sound-requested(\"select\");\n                            root.mark-saved();\n",
)
replace_once(
    "ui/app.slint",
    "            Rectangle { min-height: 14px; max-height: 14px; background: transparent; }\n\n            DisclosureRow {\n                label: \"Advanced\";\n                expanded: root.advanced-open;\n                reduced-motion: root.reduced-motion;\n                toggled => { root.advanced-open = !root.advanced-open; }\n            }\n\n            advanced-content := Rectangle {\n                min-height: root.advanced-open ? 54px : 0px;\n                max-height: root.advanced-open ? 54px : 0px;\n",
    '''            Rectangle { min-height: 10px; max-height: 10px; background: transparent; }

            DisclosureRow {
                label: "Advanced";
                expanded: root.advanced-open;
                reduced-motion: root.reduced-motion;
                toggled => {
                    root.ui-sound-requested(root.advanced-open ? "collapse" : "expand");
                    root.advanced-open = !root.advanced-open;
                }
            }

            advanced-content := Rectangle {
                min-height: root.advanced-open ? 82px : 0px;
                max-height: root.advanced-open ? 82px : 0px;
''',
)
# Replace the single Advanced row with two dense settings rows.
replace_once(
    "ui/app.slint",
    '''                HorizontalLayout {
                    padding-left: 12px;
                    padding-right: 2px;
                    spacing: 12px;
                    cross-axis-alignment: center;

                    VerticalLayout {
                        spacing: 1px;
                        Text { text: "Precision mode"; color: NumFlowTheme.label; font-size: 12px; font-weight: 500; }
                        Text { text: "Reduce pointer speed for fine movement"; color: NumFlowTheme.secondary-label; font-size: 10px; }
                    }

                    Rectangle { horizontal-stretch: 1; background: transparent; }

                    Text {
                        text: root.precision-enabled ? "On" : "Off";
                        color: NumFlowTheme.secondary-label;
                        font-size: 10px;
                    }

                    GlassToggle {
                        enabled: root.advanced-open;
                        a11y-label: "Precision mode";
                        checked <=> root.precision-enabled;
                        reduced-motion: root.reduced-motion;
                        toggled(checked) => {
                            root.precision-toggled(checked);
                            root.mark-saved();
                        }
                    }
                }
''',
    '''                VerticalLayout {
                    padding-left: 12px;
                    padding-right: 2px;
                    spacing: 2px;

                    Rectangle {
                        min-height: 40px;
                        max-height: 40px;
                        background: transparent;
                        HorizontalLayout {
                            spacing: 12px;
                            cross-axis-alignment: center;
                            VerticalLayout {
                                spacing: 1px;
                                Text { text: "Precision mode"; color: NumFlowTheme.label; font-size: 12px; font-weight: 500; }
                                Text { text: "Reduce pointer speed for fine movement"; color: NumFlowTheme.secondary-label; font-size: 10px; }
                            }
                            Rectangle { horizontal-stretch: 1; background: transparent; }
                            GlassToggle {
                                enabled: root.advanced-open;
                                a11y-label: "Precision mode";
                                checked <=> root.precision-enabled;
                                reduced-motion: root.reduced-motion;
                                toggled(checked) => {
                                    root.precision-toggled(checked);
                                    root.mark-saved();
                                }
                            }
                        }
                    }

                    Rectangle {
                        min-height: 40px;
                        max-height: 40px;
                        background: transparent;
                        HorizontalLayout {
                            spacing: 12px;
                            cross-axis-alignment: center;
                            VerticalLayout {
                                spacing: 1px;
                                Text { text: "Interface sounds"; color: NumFlowTheme.label; font-size: 12px; font-weight: 500; }
                                Text { text: "Glass feedback for meaningful state changes"; color: NumFlowTheme.secondary-label; font-size: 10px; }
                            }
                            Rectangle { horizontal-stretch: 1; background: transparent; }
                            GlassToggle {
                                enabled: root.advanced-open;
                                a11y-label: "Interface sounds";
                                checked <=> root.sounds-enabled;
                                reduced-motion: root.reduced-motion;
                                toggled(checked) => {
                                    root.sounds-toggled(checked);
                                    root.mark-saved();
                                }
                            }
                        }
                    }
                }
''',
)
replace_once(
    "ui/app.slint",
    "                            root.hud-toggled(checked);\n                            root.mark-saved();\n",
    "                            root.hud-toggled(checked);\n                            root.ui-sound-requested(checked ? \"toggle-on\" : \"toggle-off\");\n                            root.mark-saved();\n",
)
replace_once(
    "ui/app.slint",
    "                        clicked => { bindings-panel.show(); }\n",
    "                        clicked => { root.ui-sound-requested(\"open\"); bindings-panel.show(); }\n",
)
replace_once(
    "ui/app.slint",
    "                        clicked => { more-menu.show(); }\n",
    "                        clicked => { root.ui-sound-requested(\"open\"); more-menu.show(); }\n",
)
replace_once(
    "ui/app.slint",
    "                        clicked => { bindings-panel.close(); }\n",
    "                        clicked => { root.ui-sound-requested(\"close\"); bindings-panel.close(); }\n",
)
replace_once(
    "ui/app.slint",
    "                    clicked => {\n                        more-menu.close();\n                        reset-dialog.show();\n                    }\n",
    "                    clicked => {\n                        root.ui-sound-requested(\"delete\");\n                        more-menu.close();\n                        reset-dialog.show();\n                    }\n",
)
# Slightly tighten footer spacer to keep the expanded Advanced section within the compact window.
replace_once(
    "ui/app.slint",
    "            Rectangle { min-height: 10px; max-height: 10px; background: transparent; }\n\n            Rectangle {\n                min-height: 34px;\n",
    "            Rectangle { min-height: 8px; max-height: 8px; background: transparent; }\n\n            Rectangle {\n                min-height: 34px;\n",
)

# Morphicons-inspired native motion: one shape rotates/moves instead of swapping abruptly.
replace_once(
    "ui/design-system.slint",
    "        animate x {\n            duration: root.reduced-motion ? 0ms : 210ms;\n            easing: ease-out;\n        }\n",
    "        animate x {\n            duration: root.reduced-motion ? 0ms : 210ms;\n            easing: ease-out-back;\n        }\n",
)
replace_once(
    "ui/design-system.slint",
    "        animate x {\n            duration: root.reduced-motion ? 0ms : 190ms;\n            easing: ease-out;\n        }\n",
    "        animate x {\n            duration: root.reduced-motion ? 0ms : 200ms;\n            easing: ease-out-back;\n        }\n",
)
replace_once(
    "ui/design-system.slint",
    '''        Text {
            text: root.expanded ? "⌄" : "›";
            color: root.expanded ? NumFlowTheme.accent-secondary : NumFlowTheme.secondary-label;
            font-size: 17px;
            font-weight: 500;
            opacity: interaction.has-hover || root.expanded ? 1 : 0.78;
            animate opacity {
                duration: root.reduced-motion ? 0ms : 120ms;
            }
        }
''',
    '''        Text {
            text: "›";
            color: root.expanded ? NumFlowTheme.accent-secondary : NumFlowTheme.secondary-label;
            font-size: 17px;
            font-weight: 500;
            opacity: interaction.has-hover || root.expanded ? 1 : 0.78;
            transform-rotation: root.expanded ? 90deg : 0deg;
            transform-scale: interaction.pressed ? 0.92 : 1;
            animate opacity {
                duration: root.reduced-motion ? 0ms : 120ms;
            }
            animate transform-rotation {
                duration: root.reduced-motion ? 0ms : 210ms;
                easing: ease-out-back;
            }
            animate transform-scale {
                duration: root.reduced-motion ? 0ms : 120ms;
                easing: ease-out;
            }
        }
''',
)

# HUD content changes get a short spring-like icon settle while preserving the existing fade/slide.
replace_once(
    "ui/hud.slint",
    '''            Rectangle {
                min-width: 42px;
                max-width: 42px;
                min-height: 42px;
                max-height: 42px;
                border-radius: 12px;
''',
    '''            Rectangle {
                min-width: 42px;
                max-width: 42px;
                min-height: 42px;
                max-height: 42px;
                border-radius: 12px;
                transform-scale: root.revealed ? 1 : 0.88;
                transform-rotation: root.revealed ? 0deg : -5deg;
''',
)
replace_once(
    "ui/hud.slint",
    "                drop-shadow-color: #0a84ff20;\n\n                Rectangle {\n",
    '''                drop-shadow-color: #0a84ff20;

                animate transform-scale, transform-rotation {
                    duration: root.reduced-motion ? 0ms : 210ms;
                    easing: ease-out-back;
                }

                Rectangle {
''',
)

# Source/license notice for committed generated WAV assets.
Path("assets/sfx/README.md").parent.mkdir(parents=True, exist_ok=True)
Path("assets/sfx/README.md").write_text(
    """# NumFlow UI sound assets\n\n"
    "NumFlow uses selected semantic cues from the **UI SFX Glass** pack. The upstream generated "
    "audio is dedicated to the public domain under **CC0 1.0**.\n\n"
    "Source: https://github.com/romainsimon/uisfx/tree/main/packages/uisfx/sounds/glass\n\n"
    "The repository stores PCM WAV conversions (mono, 22.05 kHz, 16-bit) so the Windows native "
    "build can play them directly with `PlaySound` without a heavy audio dependency.\n\n"
    "Included semantic cues: toggle-on/off, select, open/close, expand/collapse, drag-start, "
    "release, delete, and error. Pointer motion and ordinary clicks intentionally remain silent.\n"
    """,
    encoding="utf-8",
)
