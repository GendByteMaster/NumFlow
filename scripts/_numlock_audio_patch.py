from pathlib import Path
import re


def replace_once(path: Path, old: str, new: str) -> None:
    text = path.read_text(encoding="utf-8")
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one marker, found {count}: {old[:100]!r}")
    path.write_text(text.replace(old, new, 1), encoding="utf-8")


hook = Path("crates/numflow-windows/src/hook.rs")
lib = Path("crates/numflow-windows/src/lib.rs")
runtime = Path("src/runtime.rs")
app_ui = Path("ui/app.slint")
tray_ui = Path("ui/tray.slint")
bindings_ui = Path("src/bindings_ui.rs")
audio = Path("crates/numflow-windows/src/audio.rs")

hook.write_text(r'''use std::{
    io,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender},
    },
    thread::{self, JoinHandle},
};

use crossbeam_channel::{Receiver, Sender, TrySendError};
use windows::{
    Win32::{
        Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM},
        System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentThreadId},
        UI::{
            Input::KeyboardAndMouse::VK_NUMLOCK,
            WindowsAndMessaging::{
                CallNextHookEx, GetKeyState, GetMessageW, KBDLLHOOKSTRUCT, LLKHF_EXTENDED, MSG,
                PM_NOREMOVE, PeekMessageW, PostThreadMessageW, SetWindowsHookExW,
                UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_QUIT,
                WM_SYSKEYDOWN, WM_SYSKEYUP,
            },
        },
    },
    core::Error as WindowsError,
};

use crate::{KeyState, PhysicalKeyEvent, map_numpad_key};

const DEFAULT_QUEUE_CAPACITY: usize = 128;
static EVENT_DISPATCHER: OnceLock<Mutex<Option<HookDispatcher>>> = OnceLock::new();
static INTERCEPTION_ENABLED: AtomicBool = AtomicBool::new(false);
static NUM_LOCK_ON: AtomicBool = AtomicBool::new(true);
static NUM_LOCK_KEY_DOWN: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyboardHookEvent {
    Key(PhysicalKeyEvent),
    NumLockChanged { num_lock_on: bool },
}

#[derive(Debug)]
struct HookDispatcher {
    sender: Sender<KeyboardHookEvent>,
    overflow_reader: Receiver<KeyboardHookEvent>,
}

#[derive(Debug, thiserror::Error)]
pub enum HookError {
    #[error("the NumFlow keyboard hook is already active")]
    AlreadyActive,
    #[error("failed to spawn the Windows hook thread: {0}")]
    ThreadSpawn(#[source] io::Error),
    #[error("failed to install WH_KEYBOARD_LL: {0}")]
    Install(#[source] WindowsError),
    #[error("the Windows hook message loop failed: {0}")]
    MessageLoop(#[source] WindowsError),
    #[error("failed to stop the Windows hook thread: {0}")]
    Stop(#[source] WindowsError),
    #[error("the Windows hook thread terminated unexpectedly")]
    ThreadTerminated,
    #[error("the Windows hook thread panicked")]
    ThreadPanicked,
}

#[derive(Debug)]
pub struct KeyboardHook {
    thread_id: u32,
    join: Option<JoinHandle<Result<(), HookError>>>,
}

impl KeyboardHook {
    /// Starts the global low-level keyboard hook with the default event queue capacity.
    ///
    /// # Errors
    ///
    /// Returns [`HookError`] if the hook thread cannot be spawned, the Win32 hook cannot be
    /// installed, another `NumFlow` hook is already active, or the hook thread exits before setup
    /// completes.
    pub fn start() -> Result<(Self, Receiver<KeyboardHookEvent>), HookError> {
        Self::start_with_capacity(DEFAULT_QUEUE_CAPACITY)
    }

    /// Starts the global low-level keyboard hook with a bounded event queue.
    ///
    /// A zero capacity request is normalized to a capacity of one so the hook callback never uses
    /// a rendezvous channel that could block.
    ///
    /// # Errors
    ///
    /// Returns [`HookError`] if the hook thread cannot be spawned, the Win32 hook cannot be
    /// installed, another `NumFlow` hook is already active, or the hook thread exits before setup
    /// completes.
    pub fn start_with_capacity(
        queue_capacity: usize,
    ) -> Result<(Self, Receiver<KeyboardHookEvent>), HookError> {
        let capacity = queue_capacity.max(1);
        let (event_sender, event_receiver) = crossbeam_channel::bounded(capacity);
        let event_overflow_reader = event_receiver.clone();
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);

        let join = thread::Builder::new()
            .name("numflow-keyboard-hook".to_owned())
            .spawn(move || hook_thread(event_sender, event_overflow_reader, &ready_sender))
            .map_err(HookError::ThreadSpawn)?;

        let thread_id = match ready_receiver.recv() {
            Ok(Ok(thread_id)) => thread_id,
            Ok(Err(error)) => {
                let _ = join.join();
                return Err(error);
            }
            Err(_) => {
                let _ = join.join();
                return Err(HookError::ThreadTerminated);
            }
        };

        INTERCEPTION_ENABLED.store(false, Ordering::Release);

        Ok((
            Self {
                thread_id,
                join: Some(join),
            },
            event_receiver,
        ))
    }

    #[must_use]
    pub fn interception_enabled(&self) -> bool {
        INTERCEPTION_ENABLED.load(Ordering::Acquire)
    }

    #[must_use]
    pub fn num_lock_on(&self) -> bool {
        NUM_LOCK_ON.load(Ordering::Acquire)
    }

    pub fn set_interception_enabled(&self, enabled: bool) {
        let should_intercept = enabled && !self.num_lock_on();
        INTERCEPTION_ENABLED.store(should_intercept, Ordering::Release);
    }

    pub fn emergency_disable(&self) {
        INTERCEPTION_ENABLED.store(false, Ordering::Release);
    }

    /// Disables interception, requests message-loop shutdown, and joins the hook thread.
    ///
    /// # Errors
    ///
    /// Returns [`HookError`] if posting the shutdown message fails, unhooking fails, the message
    /// loop reports a Win32 error, or the hook thread panics.
    pub fn stop(mut self) -> Result<(), HookError> {
        self.emergency_disable();
        self.request_stop()?;
        self.join_thread()
    }

    fn request_stop(&self) -> Result<(), HookError> {
        if self.join.as_ref().is_some_and(JoinHandle::is_finished) {
            return Ok(());
        }

        unsafe {
            PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0))
                .map_err(HookError::Stop)
        }
    }

    fn join_thread(&mut self) -> Result<(), HookError> {
        let Some(join) = self.join.take() else {
            return Ok(());
        };

        match join.join() {
            Ok(result) => result,
            Err(_) => Err(HookError::ThreadPanicked),
        }
    }
}

impl Drop for KeyboardHook {
    fn drop(&mut self) {
        self.emergency_disable();
        let _ = self.request_stop();
        let _ = self.join_thread();
    }
}

fn hook_thread(
    event_sender: Sender<KeyboardHookEvent>,
    event_overflow_reader: Receiver<KeyboardHookEvent>,
    ready_sender: &SyncSender<Result<u32, HookError>>,
) -> Result<(), HookError> {
    let thread_id = unsafe { GetCurrentThreadId() };
    let mut message = MSG::default();

    let _ = unsafe { PeekMessageW(&raw mut message, None, 0, 0, PM_NOREMOVE) };
    let key_state = unsafe { GetKeyState(i32::from(VK_NUMLOCK.0)) };
    NUM_LOCK_ON.store(key_state & 1 != 0, Ordering::Release);
    NUM_LOCK_KEY_DOWN.store(key_state < 0, Ordering::Release);

    let module = match unsafe { GetModuleHandleW(None) } {
        Ok(module) => module,
        Err(error) => {
            let _ = ready_sender.send(Err(HookError::Install(error)));
            return Ok(());
        }
    };

    let hook = match unsafe {
        SetWindowsHookExW(
            WH_KEYBOARD_LL,
            Some(keyboard_hook_proc),
            Some(HINSTANCE(module.0)),
            0,
        )
    } {
        Ok(hook) => hook,
        Err(error) => {
            let _ = ready_sender.send(Err(HookError::Install(error)));
            return Ok(());
        }
    };

    if !register_dispatcher(event_sender, event_overflow_reader) {
        let _ = unsafe { UnhookWindowsHookEx(hook) };
        let _ = ready_sender.send(Err(HookError::AlreadyActive));
        return Ok(());
    }

    if ready_sender.send(Ok(thread_id)).is_err() {
        clear_dispatcher();
        let _ = unsafe { UnhookWindowsHookEx(hook) };
        return Ok(());
    }

    let loop_result = run_message_loop();
    INTERCEPTION_ENABLED.store(false, Ordering::Release);
    clear_dispatcher();
    let unhook_result = unsafe { UnhookWindowsHookEx(hook) };

    loop_result?;
    unhook_result.map_err(HookError::Stop)
}

fn run_message_loop() -> Result<(), HookError> {
    let mut message = MSG::default();

    loop {
        let result = unsafe { GetMessageW(&raw mut message, None, 0, 0) };
        match result.0 {
            -1 => return Err(HookError::MessageLoop(WindowsError::from_thread())),
            0 => return Ok(()),
            _ => {}
        }
    }
}

fn register_dispatcher(
    sender: Sender<KeyboardHookEvent>,
    overflow_reader: Receiver<KeyboardHookEvent>,
) -> bool {
    let dispatcher = EVENT_DISPATCHER.get_or_init(|| Mutex::new(None));
    let Ok(mut slot) = dispatcher.lock() else {
        return false;
    };
    if slot.is_some() {
        return false;
    }
    *slot = Some(HookDispatcher {
        sender,
        overflow_reader,
    });
    true
}

fn clear_dispatcher() {
    let Some(dispatcher) = EVENT_DISPATCHER.get() else {
        return;
    };
    if let Ok(mut slot) = dispatcher.lock() {
        *slot = None;
    }
}

fn dispatch_event(event: KeyboardHookEvent, priority: bool) -> bool {
    let Some(dispatcher) = EVENT_DISPATCHER.get() else {
        return false;
    };
    let Ok(slot) = dispatcher.try_lock() else {
        return false;
    };
    let Some(dispatcher) = slot.as_ref() else {
        return false;
    };

    match dispatcher.sender.try_send(event) {
        Ok(()) => true,
        Err(TrySendError::Full(event)) if priority => {
            let _ = dispatcher.overflow_reader.try_recv();
            dispatcher.sender.try_send(event).is_ok()
        }
        Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => false,
    }
}

fn num_lock_transition(
    num_lock_on: bool,
    key_down: bool,
    state: KeyState,
) -> (bool, bool, Option<bool>) {
    match state {
        KeyState::Pressed if !key_down => {
            let next = !num_lock_on;
            (next, true, Some(next))
        }
        KeyState::Pressed => (num_lock_on, true, None),
        KeyState::Released => (num_lock_on, false, None),
    }
}

fn observe_num_lock(state: KeyState) -> Option<bool> {
    let current = NUM_LOCK_ON.load(Ordering::Acquire);
    let key_down = NUM_LOCK_KEY_DOWN.load(Ordering::Acquire);
    let (next, next_key_down, changed) = num_lock_transition(current, key_down, state);
    NUM_LOCK_ON.store(next, Ordering::Release);
    NUM_LOCK_KEY_DOWN.store(next_key_down, Ordering::Release);

    if changed == Some(true) {
        // Num Lock ON means normal number entry. Stop interception in the hook immediately;
        // the runtime will safely release any held pointer state as it consumes the mode event.
        INTERCEPTION_ENABLED.store(false, Ordering::Release);
    }

    changed
}

unsafe extern "system" fn keyboard_hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let state = match u32::try_from(wparam.0).ok() {
            Some(WM_KEYDOWN | WM_SYSKEYDOWN) => Some(KeyState::Pressed),
            Some(WM_KEYUP | WM_SYSKEYUP) => Some(KeyState::Released),
            _ => None,
        };

        if let Some(state) = state {
            let keyboard = unsafe { *(lparam.0 as *const KBDLLHOOKSTRUCT) };

            if keyboard.vkCode == u32::from(VK_NUMLOCK.0) {
                if let Some(num_lock_on) = observe_num_lock(state) {
                    let _ = dispatch_event(
                        KeyboardHookEvent::NumLockChanged { num_lock_on },
                        true,
                    );
                }
                return unsafe { CallNextHookEx(None, code, wparam, lparam) };
            }

            if INTERCEPTION_ENABLED.load(Ordering::Acquire)
                && !NUM_LOCK_ON.load(Ordering::Acquire)
            {
                let event = PhysicalKeyEvent::new(
                    keyboard.vkCode,
                    keyboard.scanCode,
                    keyboard.flags.0 & LLKHF_EXTENDED.0 != 0,
                    state,
                );

                if map_numpad_key(event).is_some()
                    && dispatch_event(KeyboardHookEvent::Key(event), false)
                {
                    return LRESULT(1);
                }
            }
        }
    }

    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::num_lock_transition;
    use crate::KeyState;

    #[test]
    fn num_lock_toggles_once_per_physical_press() {
        let (on, down, changed) = num_lock_transition(true, false, KeyState::Pressed);
        assert!(!on);
        assert!(down);
        assert_eq!(changed, Some(false));

        let (on, down, changed) = num_lock_transition(on, down, KeyState::Pressed);
        assert!(!on);
        assert!(down);
        assert_eq!(changed, None);

        let (on, down, changed) = num_lock_transition(on, down, KeyState::Released);
        assert!(!on);
        assert!(!down);
        assert_eq!(changed, None);

        let (on, down, changed) = num_lock_transition(on, down, KeyState::Pressed);
        assert!(on);
        assert!(down);
        assert_eq!(changed, Some(true));
    }
}
''', encoding="utf-8")

audio.write_text(r'''use std::{
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
        assert!(CUE_DURATION_MS < 1_000);
    }
}
''', encoding="utf-8")

replace_once(
    lib,
    "#[cfg(windows)]\nmod hook;",
    "#[cfg(windows)]\nmod audio;\n#[cfg(windows)]\nmod hook;",
)
replace_once(
    lib,
    "#[cfg(windows)]\npub use hook::{HookError, KeyboardHook};",
    "#[cfg(windows)]\npub use audio::{AudioCue, AudioFeedbackError, AudioFeedbackService};\n#[cfg(windows)]\npub use hook::{HookError, KeyboardHook, KeyboardHookEvent};",
)

# Runtime imports and hook event type.
replace_once(
    runtime,
    "        KeyState, KeyboardEventNormalizer, KeyboardHook, NormalizedKeyEvent, PhysicalKeyEvent,\n        WindowsPointer,",
    "        AudioCue, AudioFeedbackService, KeyState, KeyboardEventNormalizer, KeyboardHook,\n        KeyboardHookEvent, NormalizedKeyEvent, WindowsPointer,",
)

# Num Lock owns enable state: legacy/manual enable actions are ignored by mapped keys.
replace_once(
    runtime,
    "            if event.state == KeyState::Pressed {\n                self.apply_action(event.action)\n            } else {\n                Ok(Vec::new())\n            }",
    "            if event.state == KeyState::Pressed {\n                if matches!(\n                    event.action,\n                    InputAction::ToggleEnabled | InputAction::SetEnabled(_)\n                ) {\n                    return Ok(Vec::new());\n                }\n                self.apply_action(event.action)\n            } else {\n                Ok(Vec::new())\n            }",
)

# Startup synchronizes controller/interception to current Num Lock state before readiness.
replace_once(
    runtime,
    "        hook.set_interception_enabled(false);\n\n        let mut normalizer = KeyboardEventNormalizer::default();\n        let mut machine = RuntimeMachine::new(config, WindowsPointer::default());\n        let _ = ready_sender.send(Ok(()));",
    "        hook.set_interception_enabled(false);\n\n        let audio_feedback = match AudioFeedbackService::start() {\n            Ok(service) => Some(service),\n            Err(error) => {\n                tracing::warn!(%error, \"NumFlow audio feedback is unavailable\");\n                None\n            }\n        };\n        let mut normalizer = KeyboardEventNormalizer::default();\n        let mut machine = RuntimeMachine::new(config, WindowsPointer::default());\n        let startup_effects = match apply_num_lock_mode(&mut machine, hook.num_lock_on()) {\n            Ok(effects) => effects,\n            Err(error) => {\n                let _ = ready_sender.send(Err(error.to_string()));\n                return;\n            }\n        };\n        hook.set_interception_enabled(machine.enabled());\n        if !startup_effects.is_empty() {\n            event_sink.send(RuntimeEvent::Effects {\n                state: machine.snapshot(),\n                effects: startup_effects,\n            });\n        }\n        let _ = ready_sender.send(Ok(()));",
)

# Pass audio service into both keyboard handler calls.
text = runtime.read_text(encoding="utf-8")
old_call = '''                            &mut normalizer,
                            event_sink,
                        );
'''
new_call = '''                            &mut normalizer,
                            event_sink,
                            audio_feedback.as_ref(),
                        );
'''
# Exactly two keyboard handler calls have this tail, while command calls share a similar tail.
needle = '''                        running = handle_keyboard_message(
                            event,
                            &mut machine,
                            &hook,
                            &mut normalizer,
                            event_sink,
                        );'''
replacement = '''                        running = handle_keyboard_message(
                            event,
                            &mut machine,
                            &hook,
                            &mut normalizer,
                            event_sink,
                            audio_feedback.as_ref(),
                        );'''
if text.count(needle) != 2:
    raise SystemExit(f"runtime: expected 2 keyboard handler calls, found {text.count(needle)}")
text = text.replace(needle, replacement)
runtime.write_text(text, encoding="utf-8")

# Replace keyboard handler with HookEvent-aware implementation.
text = runtime.read_text(encoding="utf-8")
start = text.index("    fn handle_keyboard_message(")
end = text.index("\n    fn apply_command(", start)
new_handler = r'''    fn handle_keyboard_message(
        event: Result<KeyboardHookEvent, crossbeam_channel::RecvError>,
        machine: &mut RuntimeMachine<WindowsPointer>,
        hook: &KeyboardHook,
        normalizer: &mut KeyboardEventNormalizer,
        event_sink: &RuntimeEventSink,
        audio_feedback: Option<&AudioFeedbackService>,
    ) -> bool {
        let Ok(event) = event else {
            fail_safe(
                machine,
                hook,
                normalizer,
                event_sink,
                "keyboard hook event channel disconnected",
            );
            return false;
        };

        let event = match event {
            KeyboardHookEvent::NumLockChanged { num_lock_on } => {
                normalizer.reset();
                if let Some(audio_feedback) = audio_feedback {
                    audio_feedback.play(if num_lock_on {
                        AudioCue::NumFlowOff
                    } else {
                        AudioCue::NumFlowOn
                    });
                }

                match apply_num_lock_mode(machine, num_lock_on) {
                    Ok(effects) => {
                        hook.set_interception_enabled(machine.enabled());
                        event_sink.send(RuntimeEvent::Effects {
                            state: machine.snapshot(),
                            effects,
                        });
                    }
                    Err(error) => {
                        fail_safe(machine, hook, normalizer, event_sink, &error.to_string());
                    }
                }
                return true;
            }
            KeyboardHookEvent::Key(event) => event,
        };

        let Some(normalized_event) = normalizer.process(event, &machine.bindings) else {
            return true;
        };

        match machine.handle_key_event(normalized_event) {
            Ok(effects) => {
                hook.set_interception_enabled(machine.enabled());
                if !effects.is_empty() {
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
'''
text = text[:start] + new_handler + text[end:]
runtime.write_text(text, encoding="utf-8")

# Manual UI/runtime commands can change settings, but not the Num-Lock-owned enable state.
replace_once(
    runtime,
    "            RuntimeCommand::Apply(action) => {\n                machine\n                    .apply_action(action)\n                    .map_err(|error| error.to_string())?;",
    "            RuntimeCommand::Apply(action) => {\n                if matches!(action, InputAction::ToggleEnabled | InputAction::SetEnabled(_)) {\n                    return Ok(());\n                }\n                machine\n                    .apply_action(action)\n                    .map_err(|error| error.to_string())?;",
)

# Generic helper keeps Num Lock state transition directly testable with MockPointer.
insert_before = "    fn fail_safe(\n"
helper = '''    fn apply_num_lock_mode<B: PointerBackend>(
        machine: &mut RuntimeMachine<B>,
        num_lock_on: bool,
    ) -> Result<Vec<CoreEffect>, B::Error> {
        machine.motion.stop();
        machine.apply_action(InputAction::SetEnabled(!num_lock_on))
    }

'''
text = runtime.read_text(encoding="utf-8")
if text.count(insert_before) != 1:
    raise SystemExit("runtime: fail_safe marker missing")
text = text.replace(insert_before, helper + insert_before, 1)
runtime.write_text(text, encoding="utf-8")

# Test import + Num Lock safety tests.
replace_once(
    runtime,
    "        use super::{RuntimeEventSink, RuntimeMachine};",
    "        use super::{RuntimeEventSink, RuntimeMachine, apply_num_lock_mode};",
)
text = runtime.read_text(encoding="utf-8")
marker = '''        #[test]
        fn changing_bindings_stops_existing_motion() {
'''
addition = r'''        #[test]
        fn num_lock_on_disables_runtime_and_releases_drag() {
            let mut machine = runtime_machine();
            apply_num_lock_mode(&mut machine, false).expect("mock is infallible");
            machine
                .apply_action(InputAction::Hold)
                .expect("mock is infallible");
            machine.motion.press(Direction::Right);
            assert!(machine.enabled());
            assert_eq!(machine.pointer.held, vec![MouseButton::Left]);

            let effects = apply_num_lock_mode(&mut machine, true).expect("mock is infallible");

            assert!(!machine.enabled());
            assert!(!machine.motion.is_moving());
            assert!(machine.pointer.held.is_empty());
            assert!(effects.iter().any(|effect| matches!(
                effect,
                CoreEffect::State(StateChange::Enabled(false))
            )));
        }

        #[test]
        fn num_lock_off_enables_runtime() {
            let mut machine = runtime_machine();
            let effects = apply_num_lock_mode(&mut machine, false).expect("mock is infallible");

            assert!(machine.enabled());
            assert!(effects.iter().any(|effect| matches!(
                effect,
                CoreEffect::State(StateChange::Enabled(true))
            )));
        }

        #[test]
        fn mapped_enable_actions_do_not_override_num_lock_mode() {
            let mut machine = runtime_machine();
            apply_num_lock_mode(&mut machine, false).expect("mock is infallible");

            let effects = machine
                .handle_key_event(NormalizedKeyEvent {
                    key: NumpadKey::Num5,
                    action: InputAction::SetEnabled(false),
                    state: KeyState::Pressed,
                    repeated: false,
                })
                .expect("mock is infallible");

            assert!(effects.is_empty());
            assert!(machine.enabled());
        }

'''
if text.count(marker) != 1:
    raise SystemExit("runtime: changing bindings marker missing")
text = text.replace(marker, addition + marker, 1)
runtime.write_text(text, encoding="utf-8")

# Test module needs CoreEffect/StateChange imports for new safety assertions.
replace_once(
    runtime,
    "            Bindings, Direction, InputAction, MotionConfig, MouseButton, NumpadKey, PointerBackend,",
    "            Bindings, CoreEffect, Direction, InputAction, MotionConfig, MouseButton, NumpadKey,\n            PointerBackend, StateChange,",
)

# Remove enable/disable binding choices now that Num Lock is the authoritative mode switch.
replace_once(
    bindings_ui,
    "pub(crate) const BINDING_CHOICES: [BindingChoice; 21] = [",
    "pub(crate) const BINDING_CHOICES: [BindingChoice; 18] = [",
)
for line in [
    "    BindingChoice::Action(InputActionConfig::ToggleEnabled),\n",
    "    BindingChoice::Action(InputActionConfig::SetEnabled(true)),\n",
    "    BindingChoice::Action(InputActionConfig::SetEnabled(false)),\n",
]:
    text = bindings_ui.read_text(encoding="utf-8")
    if text.count(line) != 1:
        raise SystemExit(f"bindings_ui: expected enable action line {line!r}")
    bindings_ui.write_text(text.replace(line, "", 1), encoding="utf-8")

# Make the existing status control read-only; Num Lock is now the source of truth.
replace_once(
    app_ui,
    '''            Switch {
                text: root.numflow-enabled ? "On" : "Off";
                checked <=> root.numflow-enabled;
                toggled => {
                    root.enabled-toggled(self.checked);
                }
            }
''',
    '''            Switch {
                text: root.numflow-enabled ? "On" : "Off";
                checked: root.numflow-enabled;
                enabled: false;
            }
''',
)
replace_once(
    app_ui,
    '                    title: "NumFlow";\n                    value: root.numflow-enabled ? "On" : "Off";',
    '                    title: "NumFlow · Num Lock controlled";\n                    value: root.numflow-enabled ? "On" : "Off";',
)

# Tray status is also read-only and documents the source of truth.
replace_once(
    tray_ui,
    '''    tooltip: root.numflow-enabled
        ? "NumFlow · On · Running in background"
        : "NumFlow · Off · Running in background";''',
    '''    tooltip: root.numflow-enabled
        ? "NumFlow · On · Num Lock controlled · Running in background"
        : "NumFlow · Off · Num Lock controlled · Running in background";''',
)
replace_once(
    tray_ui,
    '''        MenuItem {
            title: root.numflow-enabled ? "NumFlow On" : "NumFlow Off";
            checkable: true;
            checked: root.numflow-enabled;
            activated => {
                root.numflow-enabled = !root.numflow-enabled;
                root.enabled-toggled(root.numflow-enabled);
            }
        }
''',
    '''        MenuItem {
            title: root.numflow-enabled ? "NumFlow On · Num Lock controlled" : "NumFlow Off · Num Lock controlled";
            checkable: true;
            checked: root.numflow-enabled;
            enabled: false;
        }
''',
)

# Sanity assertions.
final_hook = hook.read_text(encoding="utf-8")
final_runtime = runtime.read_text(encoding="utf-8")
if "NumLockChanged" not in final_hook or "NUM_LOCK_ON" not in final_hook:
    raise SystemExit("Num Lock hook mode event not installed")
if "AudioFeedbackService::start" not in final_runtime:
    raise SystemExit("audio feedback service not wired into runtime")
if "InputAction::ToggleEnabled | InputAction::SetEnabled(_)" not in final_runtime:
    raise SystemExit("legacy enable actions are not guarded")
print("Num Lock mode + non-blocking audio feedback patch applied")
