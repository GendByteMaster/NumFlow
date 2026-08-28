use std::time::{Duration, Instant};

use crossbeam_channel::{select, tick};
use numflow_core::{
    ClickKind, ControllerState, CoreEffect, InputAction, MotionEngine, MotionModifiers, NumpadKey,
    PointerBackend, PointerEffect,
};

use crate::{
    DesktopKind, KeyState, KeyboardEventNormalizer, KeyboardHook, KeyboardHookEvent, PointerError,
    SecureSettings, WindowsPointer, assistive_technology_registered, current_desktop_kind,
    current_process_integrity, current_process_ui_access, current_thread_owns_input_desktop,
    mouse_hold_active,
};

const MOTION_TICK: Duration = Duration::from_millis(8);
const DESKTOP_GUARD_TICK: Duration = Duration::from_millis(50);

#[derive(Debug, thiserror::Error)]
pub enum SecureRuntimeError {
    #[error("secure runtime refused a desktop that was not activated by registered Ease of Access")]
    UnmanagedDesktop,
    #[error("failed to start the secure keyboard hook: {0}")]
    Hook(#[from] crate::HookError),
    #[error("secure pointer injection failed: {0}")]
    Pointer(#[from] PointerError),
}

/// Runs the minimal Windows-managed accessibility input process until its desktop loses input.
///
/// This function has no UI, tray, audio, shell, updater, telemetry, or arbitrary command surface.
/// The caller is expected to be `numflow-secure.exe`, started by Ease of Access on a protected
/// desktop with `--secure-runtime`.
///
/// # Errors
///
/// Returns startup and pointer failures. Desktop loss is a normal, clean exit.
pub fn run_secure_runtime(settings: &SecureSettings) -> Result<(), SecureRuntimeError> {
    let desktop = current_desktop_kind();
    if !assistive_technology_registered()
        || !matches!(
            desktop,
            DesktopKind::Secure | DesktopKind::Locked | DesktopKind::Logon
        )
        || !current_thread_owns_input_desktop()
    {
        return Err(SecureRuntimeError::UnmanagedDesktop);
    }

    let (hook, receiver) = KeyboardHook::start()?;
    let mut machine = SecureMachine::new(settings);
    let mut normalizer = KeyboardEventNormalizer::default();
    machine.set_num_lock_mode(hook.num_lock_on())?;
    hook.set_interception_enabled(machine.enabled());
    log_secure_snapshot("startup", &hook, desktop);

    let motion_tick = tick(MOTION_TICK);
    let desktop_guard_tick = tick(DESKTOP_GUARD_TICK);
    let mut previous_tick = Instant::now();

    loop {
        select! {
            recv(receiver) -> event => {
                let Ok(event) = event else {
                    machine.fail_safe();
                    hook.emergency_disable();
                    break;
                };
                handle_keyboard_event(event, &mut machine, &hook, &mut normalizer)?;
            }
            recv(motion_tick) -> _ => {
                let now = Instant::now();
                let elapsed = now.saturating_duration_since(previous_tick);
                previous_tick = now;
                if machine.tick(elapsed).is_err() {
                    machine.stop_motion();
                }
            }
            recv(desktop_guard_tick) -> _ => {
                if !current_thread_owns_input_desktop() {
                    machine.fail_safe();
                    normalizer.reset();
                    hook.emergency_disable();
                    let _ = hook.suspend_for_desktop_switch();
                    break;
                }
            }
        }
    }

    machine.shutdown()?;
    log_secure_snapshot("shutdown", &hook, current_desktop_kind());
    hook.stop()?;
    Ok(())
}

fn handle_keyboard_event(
    event: KeyboardHookEvent,
    machine: &mut SecureMachine,
    hook: &KeyboardHook,
    normalizer: &mut KeyboardEventNormalizer,
) -> Result<(), SecureRuntimeError> {
    let event = match event {
        KeyboardHookEvent::InputUnavailable { .. } => {
            machine.fail_safe();
            normalizer.reset();
            hook.emergency_disable();
            return Ok(());
        }
        KeyboardHookEvent::NumLockChanged {
            num_lock_on,
            sync_system,
            ..
        } => {
            normalizer.reset();
            machine.set_num_lock_mode(num_lock_on)?;
            if sync_system && !hook.sync_num_lock_to_windows() {
                machine.fail_safe();
                hook.emergency_disable();
                return Ok(());
            }
            hook.set_interception_enabled(machine.enabled());
            return Ok(());
        }
        KeyboardHookEvent::Key(event) => {
            hook.record_runtime_numpad_event();
            event
        }
    };

    if let Some(event) = normalizer.process(event, &machine.bindings) {
        machine.handle_key_event(event)?;
        hook.set_interception_enabled(machine.enabled());
    }
    Ok(())
}

struct SecureMachine {
    configured_enabled: bool,
    controller: ControllerState,
    motion: MotionEngine,
    bindings: numflow_core::Bindings,
    pointer: WindowsPointer,
}

impl SecureMachine {
    fn new(settings: &SecureSettings) -> Self {
        let mut controller = ControllerState::default();
        let _ = controller.apply(InputAction::SelectButton(settings.selected_button));
        let _ = controller.apply(InputAction::SetPrecision(settings.precision));
        Self {
            configured_enabled: settings.enabled,
            controller,
            motion: MotionEngine::new(settings.motion),
            bindings: settings.bindings.clone(),
            pointer: WindowsPointer::default(),
        }
    }

    fn enabled(&self) -> bool {
        self.controller.is_enabled()
    }

    fn set_num_lock_mode(&mut self, num_lock_on: bool) -> Result<(), PointerError> {
        self.motion.stop();
        self.apply_action(InputAction::SetEnabled(
            self.configured_enabled && !num_lock_on,
        ))
    }

    fn apply_action(&mut self, action: InputAction) -> Result<(), PointerError> {
        match action {
            InputAction::Hold => return self.hold_selected_button(),
            InputAction::Release => return self.release_tracked_buttons(),
            InputAction::SetEnabled(false) if self.controller.is_enabled() => {
                self.motion.stop();
                self.release_tracked_buttons()?;
            }
            _ => {}
        }

        let effects = self.controller.apply(action);
        if !self.controller.is_enabled() {
            self.motion.stop();
        }
        self.execute_effects(&effects)
    }

    fn hold_selected_button(&mut self) -> Result<(), PointerError> {
        if !self.controller.is_enabled() || self.controller.held_button().is_some() {
            return Ok(());
        }
        self.pointer.release_all()?;
        self.pointer
            .button_down(self.controller.selected_button())?;
        let _ = self.controller.apply(InputAction::Hold);
        Ok(())
    }

    fn release_tracked_buttons(&mut self) -> Result<(), PointerError> {
        self.pointer.release_all()?;
        let _ = self.controller.apply(InputAction::Release);
        Ok(())
    }

    fn handle_key_event(&mut self, event: crate::NormalizedKeyEvent) -> Result<(), PointerError> {
        if let InputAction::Move(direction) = event.action {
            match event.state {
                KeyState::Pressed if self.controller.is_enabled() => self.motion.press(direction),
                KeyState::Released => self.motion.release(direction),
                KeyState::Pressed => {}
            }
            return Ok(());
        }

        if event.state != KeyState::Pressed {
            return Ok(());
        }
        if event.key == NumpadKey::Num0 && event.action == InputAction::Hold {
            return self.hold_selected_button();
        }
        if matches!(
            event.key,
            NumpadKey::Num5 | NumpadKey::Add | NumpadKey::Decimal
        ) {
            let had_hold = self.controller.held_button().is_some();
            self.release_tracked_buttons()?;
            if had_hold || event.key == NumpadKey::Decimal {
                return Ok(());
            }
        }
        if matches!(
            event.action,
            InputAction::ToggleEnabled | InputAction::SetEnabled(_)
        ) {
            return Ok(());
        }
        self.apply_action(event.action)
    }

    fn tick(&mut self, elapsed: Duration) -> Result<(), PointerError> {
        if !self.controller.is_enabled() {
            self.motion.stop();
            return Ok(());
        }
        let modifiers = MotionModifiers {
            precision: self.controller.is_precision_enabled(),
            boost: false,
        };
        if let Some(step) = self.motion.tick(elapsed, modifiers) {
            self.pointer.move_relative(step.dx, step.dy)?;
        }
        Ok(())
    }

    fn stop_motion(&mut self) {
        self.motion.stop();
    }

    fn execute_effects(&mut self, effects: &[CoreEffect]) -> Result<(), PointerError> {
        for effect in effects {
            let CoreEffect::Pointer(effect) = effect else {
                continue;
            };
            match effect {
                PointerEffect::Move(_) => {}
                PointerEffect::Click { button, kind } => match kind {
                    ClickKind::Single => self.pointer.click(*button)?,
                    ClickKind::Double => self.pointer.double_click(*button)?,
                },
                PointerEffect::ButtonDown(button) => self.pointer.button_down(*button)?,
                PointerEffect::ButtonUp(button) => self.pointer.button_up(*button)?,
            }
        }
        Ok(())
    }

    fn fail_safe(&mut self) {
        self.motion.stop();
        let effects = self.controller.shutdown();
        let _ = self.execute_effects(&effects);
        let _ = self.pointer.release_all();
    }

    fn shutdown(&mut self) -> Result<(), PointerError> {
        self.motion.stop();
        self.pointer.release_all()?;
        let _ = self.controller.shutdown();
        Ok(())
    }
}

fn log_secure_snapshot(reason: &str, hook: &KeyboardHook, desktop: DesktopKind) {
    let diagnostics = hook.diagnostics();
    eprintln!(
        "NumFlow: secure snapshot (reason={reason}, desktop={}, runtime=secure, integrity={}, hook_generation={}, hook_active={}, numpad_callbacks={}, numpad_dispatched={}, numpad_dropped={}, runtime_numpad_events={}, mouse_hold={}, at_registered={}, uiaccess={})",
        desktop.label(),
        current_process_integrity().unwrap_or("unknown"),
        diagnostics.hook_generation,
        diagnostics.hook_active,
        diagnostics.numpad_callbacks,
        diagnostics.numpad_dispatched,
        diagnostics.numpad_dropped,
        diagnostics.runtime_numpad_events,
        mouse_hold_active(),
        assistive_technology_registered(),
        current_process_ui_access().unwrap_or(false),
    );
}
