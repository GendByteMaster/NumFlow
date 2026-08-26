use std::{
    io,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
    },
    thread::{self, JoinHandle},
};

use windows::{
    Win32::{
        Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM},
        System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentThreadId},
        UI::WindowsAndMessaging::{
            CallNextHookEx, GetMessageW, KBDLLHOOKSTRUCT, LLKHF_EXTENDED, MSG, PM_NOREMOVE,
            PeekMessageW, PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx,
            WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP,
        },
    },
    core::Error as WindowsError,
};

use crate::{KeyState, PhysicalKeyEvent, map_numpad_key};

const DEFAULT_QUEUE_CAPACITY: usize = 128;
static EVENT_SENDER: OnceLock<Mutex<Option<SyncSender<PhysicalKeyEvent>>>> = OnceLock::new();
static INTERCEPTION_ENABLED: AtomicBool = AtomicBool::new(false);

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
    pub fn start() -> Result<(Self, Receiver<PhysicalKeyEvent>), HookError> {
        Self::start_with_capacity(DEFAULT_QUEUE_CAPACITY)
    }

    pub fn start_with_capacity(
        queue_capacity: usize,
    ) -> Result<(Self, Receiver<PhysicalKeyEvent>), HookError> {
        let capacity = queue_capacity.max(1);
        let (event_sender, event_receiver) = mpsc::sync_channel(capacity);
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);

        let join = thread::Builder::new()
            .name("numflow-keyboard-hook".to_owned())
            .spawn(move || hook_thread(event_sender, ready_sender))
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

        INTERCEPTION_ENABLED.store(true, Ordering::Release);

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

    pub fn set_interception_enabled(&self, enabled: bool) {
        INTERCEPTION_ENABLED.store(enabled, Ordering::Release);
    }

    pub fn emergency_disable(&self) {
        self.set_interception_enabled(false);
    }

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
    event_sender: SyncSender<PhysicalKeyEvent>,
    ready_sender: SyncSender<Result<u32, HookError>>,
) -> Result<(), HookError> {
    let thread_id = unsafe { GetCurrentThreadId() };
    let mut message = MSG::default();

    unsafe {
        PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE);
    }

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

    if !register_sender(event_sender) {
        let _ = unsafe { UnhookWindowsHookEx(hook) };
        let _ = ready_sender.send(Err(HookError::AlreadyActive));
        return Ok(());
    }

    if ready_sender.send(Ok(thread_id)).is_err() {
        clear_sender();
        let _ = unsafe { UnhookWindowsHookEx(hook) };
        return Ok(());
    }

    let loop_result = run_message_loop();
    INTERCEPTION_ENABLED.store(false, Ordering::Release);
    clear_sender();
    let unhook_result = unsafe { UnhookWindowsHookEx(hook) };

    loop_result?;
    unhook_result.map_err(HookError::Stop)
}

fn run_message_loop() -> Result<(), HookError> {
    let mut message = MSG::default();

    loop {
        let result = unsafe { GetMessageW(&mut message, None, 0, 0) };
        match result.0 {
            -1 => return Err(HookError::MessageLoop(WindowsError::from_win32())),
            0 => return Ok(()),
            _ => {}
        }
    }
}

fn register_sender(sender: SyncSender<PhysicalKeyEvent>) -> bool {
    let dispatcher = EVENT_SENDER.get_or_init(|| Mutex::new(None));
    let Ok(mut slot) = dispatcher.lock() else {
        return false;
    };
    if slot.is_some() {
        return false;
    }
    *slot = Some(sender);
    true
}

fn clear_sender() {
    let Some(dispatcher) = EVENT_SENDER.get() else {
        return;
    };
    if let Ok(mut slot) = dispatcher.lock() {
        *slot = None;
    }
}

fn dispatch_event(event: PhysicalKeyEvent) -> bool {
    let Some(dispatcher) = EVENT_SENDER.get() else {
        return false;
    };
    let Ok(slot) = dispatcher.try_lock() else {
        return false;
    };
    let Some(sender) = slot.as_ref() else {
        return false;
    };

    match sender.try_send(event) {
        Ok(()) => true,
        Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => false,
    }
}

unsafe extern "system" fn keyboard_hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && INTERCEPTION_ENABLED.load(Ordering::Acquire) {
        let state = match u32::try_from(wparam.0).ok() {
            Some(WM_KEYDOWN | WM_SYSKEYDOWN) => Some(KeyState::Pressed),
            Some(WM_KEYUP | WM_SYSKEYUP) => Some(KeyState::Released),
            _ => None,
        };

        if let Some(state) = state {
            let keyboard = unsafe { *(lparam.0 as *const KBDLLHOOKSTRUCT) };
            let event = PhysicalKeyEvent::new(
                keyboard.vkCode,
                keyboard.scanCode,
                keyboard.flags.0 & LLKHF_EXTENDED.0 != 0,
                state,
            );

            if map_numpad_key(event).is_some() && dispatch_event(event) {
                return LRESULT(1);
            }
        }
    }

    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}
