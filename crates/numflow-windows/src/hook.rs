use std::{
    io,
    mem::size_of,
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
            Input::KeyboardAndMouse::{
                GetKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
                KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, SendInput, VK_NUMLOCK,
            },
            WindowsAndMessaging::{
                CallNextHookEx, GetMessageW, KBDLLHOOKSTRUCT, LLKHF_EXTENDED, LLKHF_INJECTED, MSG,
                PM_NOREMOVE, PeekMessageW, PostThreadMessageW, SetWindowsHookExW,
                UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN,
                WM_SYSKEYUP,
            },
        },
    },
    core::Error as WindowsError,
};

use crate::{KeyState, PhysicalKeyEvent, map_numpad_key};

const DEFAULT_QUEUE_CAPACITY: usize = 128;
const NUMFLOW_NUM_LOCK_INJECTION_TAG: usize = 0x4E46_4E4C;
static EVENT_DISPATCHER: OnceLock<Mutex<Option<HookDispatcher>>> = OnceLock::new();
static INTERCEPTION_ENABLED: AtomicBool = AtomicBool::new(false);
static NUM_LOCK_ON: AtomicBool = AtomicBool::new(true);
static NUM_LOCK_KEY_DOWN: AtomicBool = AtomicBool::new(false);
static NUM_LOCK_REPLAY_FALLBACK: AtomicBool = AtomicBool::new(false);

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
    NUM_LOCK_REPLAY_FALLBACK.store(false, Ordering::Release);

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
    NUM_LOCK_REPLAY_FALLBACK.store(false, Ordering::Release);
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

fn dispatch_num_lock_change(state: KeyState) -> Option<bool> {
    let changed = observe_num_lock(state);
    if let Some(num_lock_on) = changed {
        let _ = dispatch_event(KeyboardHookEvent::NumLockChanged { num_lock_on }, true);
    }
    changed
}

fn num_lock_replay_inputs() -> [INPUT; 2] {
    [
        num_lock_keyboard_input(KEYEVENTF_EXTENDEDKEY),
        num_lock_keyboard_input(KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP),
    ]
}

fn num_lock_keyboard_input(flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VK_NUMLOCK,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: NUMFLOW_NUM_LOCK_INJECTION_TAG,
            },
        },
    }
}

fn replay_num_lock_to_windows() -> bool {
    let inputs = num_lock_replay_inputs();
    let input_size = i32::try_from(size_of::<INPUT>()).expect("INPUT size fits in i32");
    let inserted = unsafe { SendInput(&inputs, input_size) };
    inserted == u32::try_from(inputs.len()).expect("Num Lock replay batch length fits in u32")
}

fn is_numflow_num_lock_replay(keyboard: KBDLLHOOKSTRUCT) -> bool {
    keyboard.flags.0 & LLKHF_INJECTED.0 != 0
        && keyboard.dwExtraInfo == NUMFLOW_NUM_LOCK_INJECTION_TAG
}

fn intercept_physical_num_lock(state: KeyState) -> bool {
    let fallback = NUM_LOCK_REPLAY_FALLBACK.load(Ordering::Acquire);
    let changed = dispatch_num_lock_change(state);

    if fallback {
        if state == KeyState::Released {
            NUM_LOCK_REPLAY_FALLBACK.store(false, Ordering::Release);
        }
        return false;
    }

    if changed.is_some() && state == KeyState::Pressed && !replay_num_lock_to_windows() {
        // If SendInput cannot replay the Num Lock press, pass this physical key sequence through
        // until release. That keeps Windows' toggle state and LED synchronized instead of leaving
        // NumFlow and the OS in different modes.
        NUM_LOCK_REPLAY_FALLBACK.store(true, Ordering::Release);
        return false;
    }

    true
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
                if is_numflow_num_lock_replay(keyboard) {
                    return unsafe { CallNextHookEx(None, code, wparam, lparam) };
                }

                if keyboard.flags.0 & LLKHF_INJECTED.0 != 0 {
                    // Respect Num Lock changes injected by other software and mirror them into
                    // NumFlow state, but do not consume somebody else's injected input.
                    let _ = dispatch_num_lock_change(state);
                    return unsafe { CallNextHookEx(None, code, wparam, lparam) };
                }

                if intercept_physical_num_lock(state) {
                    return LRESULT(1);
                }

                return unsafe { CallNextHookEx(None, code, wparam, lparam) };
            }

            if INTERCEPTION_ENABLED.load(Ordering::Acquire) && !NUM_LOCK_ON.load(Ordering::Acquire)
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
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT_KEYBOARD, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, VK_NUMLOCK,
    };

    use super::{NUMFLOW_NUM_LOCK_INJECTION_TAG, num_lock_replay_inputs, num_lock_transition};
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

    #[test]
    fn num_lock_replay_is_tagged_keyboard_input() {
        let [down, up] = num_lock_replay_inputs();
        assert_eq!(down.r#type, INPUT_KEYBOARD);
        assert_eq!(up.r#type, INPUT_KEYBOARD);

        let down = unsafe { down.Anonymous.ki };
        let up = unsafe { up.Anonymous.ki };

        assert_eq!(down.wVk, VK_NUMLOCK);
        assert_eq!(up.wVk, VK_NUMLOCK);
        assert_eq!(down.dwFlags, KEYEVENTF_EXTENDEDKEY);
        assert_eq!(
            up.dwFlags,
            KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP
        );
        assert_eq!(down.dwExtraInfo, NUMFLOW_NUM_LOCK_INJECTION_TAG);
        assert_eq!(up.dwExtraInfo, NUMFLOW_NUM_LOCK_INJECTION_TAG);
    }
}
