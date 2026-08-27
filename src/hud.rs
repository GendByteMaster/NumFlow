use std::{cell::Cell, rc::Rc, time::Duration};

use numflow_core::{CoreEffect, MouseButton, PointerEffect, StateChange};
use slint::{ComponentHandle, Timer, TimerMode, winit_030::WinitWindowAccessor};

#[cfg(windows)]
use slint::winit_030::winit::{
    platform::windows::{BackdropType, WindowExtWindows},
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
};

use crate::{HudIconKind, HudWindow};

const HUD_AUTO_HIDE: Duration = Duration::from_millis(1_600);
const HUD_MARGIN_PX: u32 = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HudEvent {
    NumFlowEnabled(bool),
    ButtonSelected(MouseButton),
    Precision(bool),
    Dragging(MouseButton),
    HudEnabled,
    DefaultsRestored,
}

struct HudPresentation {
    headline: &'static str,
    detail: &'static str,
    icon: HudIconKind,
    persistent: bool,
}

impl HudPresentation {
    fn from_event(event: HudEvent) -> Self {
        match event {
            HudEvent::NumFlowEnabled(true) => Self {
                headline: "NumFlow on",
                detail: "NumPad pointer control enabled",
                icon: HudIconKind::PowerOn,
                persistent: false,
            },
            HudEvent::NumFlowEnabled(false) => Self {
                headline: "NumFlow paused",
                detail: "Pointer control disabled",
                icon: HudIconKind::PowerOff,
                persistent: false,
            },
            HudEvent::ButtonSelected(button) => Self {
                headline: button_headline(button),
                detail: "Mouse button selected",
                icon: button_icon(button),
                persistent: false,
            },
            HudEvent::Precision(true) => Self {
                headline: "Precision on",
                detail: "Reduced pointer speed",
                icon: HudIconKind::Precision,
                persistent: false,
            },
            HudEvent::Precision(false) => Self {
                headline: "Precision off",
                detail: "Normal pointer speed",
                icon: HudIconKind::Precision,
                persistent: false,
            },
            HudEvent::Dragging(button) => Self {
                headline: dragging_headline(button),
                detail: "Drag lock active · press . to release",
                icon: HudIconKind::Dragging,
                persistent: true,
            },
            HudEvent::HudEnabled => Self {
                headline: "HUD enabled",
                detail: "NumFlow feedback is visible",
                icon: HudIconKind::Info,
                persistent: false,
            },
            HudEvent::DefaultsRestored => Self {
                headline: "Defaults restored",
                detail: "Pointer and HUD settings reset",
                icon: HudIconKind::Info,
                persistent: false,
            },
        }
    }
}

pub struct HudController {
    window: HudWindow,
    hide_timer: Timer,
    enabled: bool,
    persistent: Rc<Cell<bool>>,
}

impl HudController {
    pub fn new() -> Result<Self, slint::PlatformError> {
        Ok(Self {
            window: HudWindow::new()?,
            hide_timer: Timer::default(),
            enabled: true,
            persistent: Rc::new(Cell::new(false)),
        })
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.hide_timer.stop();
            self.persistent.set(false);
            self.hide_window();
        }
    }

    pub fn set_reduced_motion(&mut self, reduced_motion: bool) {
        self.window.set_reduced_motion(reduced_motion);
    }

    pub fn observe_effects(&mut self, effects: &[CoreEffect]) {
        for effect in effects {
            match *effect {
                CoreEffect::Pointer(PointerEffect::ButtonDown(button)) => {
                    self.show_dragging(button);
                }
                CoreEffect::Pointer(PointerEffect::ButtonUp(_)) => {
                    self.clear_dragging();
                }
                CoreEffect::State(StateChange::Enabled(enabled)) => {
                    self.show_event(HudEvent::NumFlowEnabled(enabled));
                }
                CoreEffect::State(StateChange::Precision(enabled)) => {
                    self.show_event(HudEvent::Precision(enabled));
                }
                CoreEffect::State(StateChange::SelectedButton(button)) => {
                    self.show_event(HudEvent::ButtonSelected(button));
                }
                CoreEffect::Pointer(PointerEffect::Move(_) | PointerEffect::Click { .. }) => {}
            }
        }
    }

    pub fn sync_held_button(&mut self, held_button: Option<MouseButton>) {
        if !self.enabled {
            return;
        }

        if let Some(button) = held_button {
            self.show_dragging(button);
        } else if self.persistent.get() {
            self.clear_dragging();
        }
    }

    pub fn show_event(&mut self, event: HudEvent) {
        if !self.enabled {
            return;
        }

        let presentation = HudPresentation::from_event(event);
        if self.persistent.get() && !presentation.persistent {
            return;
        }

        self.present(&presentation);
    }

    fn show_dragging(&mut self, button: MouseButton) {
        self.show_event(HudEvent::Dragging(button));
    }

    fn clear_dragging(&mut self) {
        self.hide_timer.stop();
        self.persistent.set(false);
        self.window.set_persistent(false);
        self.hide_window();
    }

    fn present(&mut self, presentation: &HudPresentation) {
        self.window.set_revealed(false);
        self.window.set_headline(presentation.headline.into());
        self.window.set_detail(presentation.detail.into());
        self.window.set_icon_kind(presentation.icon);
        self.window.set_persistent(presentation.persistent);
        self.persistent.set(presentation.persistent);

        if let Err(error) = self.window.show() {
            tracing::warn!(%error, "failed to show NumFlow HUD");
            return;
        }

        self.configure_window_after_show();

        if presentation.persistent {
            self.hide_timer.stop();
        } else {
            self.start_auto_hide();
        }
    }

    fn start_auto_hide(&self) {
        let weak_window = self.window.as_weak();
        let persistent = Rc::clone(&self.persistent);

        self.hide_timer
            .start(TimerMode::SingleShot, HUD_AUTO_HIDE, move || {
                if persistent.get() {
                    return;
                }

                if let Some(window) = weak_window.upgrade()
                    && let Err(error) = window.hide()
                {
                    tracing::warn!(%error, "failed to auto-hide NumFlow HUD");
                }
            });
    }

    fn hide_window(&self) {
        self.window.set_revealed(false);
        if let Err(error) = self.window.hide() {
            tracing::warn!(%error, "failed to hide NumFlow HUD");
        }
    }

    fn configure_window_after_show(&self) {
        let weak_window = self.window.as_weak();

        Timer::single_shot(Duration::ZERO, move || {
            let Some(window) = weak_window.upgrade() else {
                return;
            };

            let configured = window.window().with_winit_window(|winit_window| {
                if let Err(error) = winit_window.set_cursor_hittest(false) {
                    tracing::warn!(%error, "failed to make NumFlow HUD click-through");
                }

                configure_native_hud_window(winit_window);
                position_hud_window(winit_window);
            });

            if configured.is_none() {
                tracing::warn!(
                    "NumFlow HUD requires the Slint winit backend for overlay window behavior"
                );
            }
            window.set_revealed(true);
        });
    }
}

#[cfg(windows)]
fn configure_native_hud_window(winit_window: &slint::winit_030::winit::window::Window) {
    // Winit maintains the shell-facing skip-taskbar state (including Explorer restarts).
    winit_window.set_skip_taskbar(true);
    // TransientWindow maps to the Windows Background Acrylic system backdrop when available.
    // Older Windows versions keep the Slint translucent material fallback.
    winit_window.set_system_backdrop(BackdropType::TransientWindow);

    let handle = match winit_window.window_handle() {
        Ok(handle) => handle,
        Err(error) => {
            tracing::warn!(%error, "failed to obtain native handle for NumFlow HUD");
            return;
        }
    };

    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        tracing::warn!("NumFlow HUD returned a non-Win32 window handle on Windows");
        return;
    };

    if let Err(error) = numflow_windows::configure_hud_native_window(handle.hwnd.get()) {
        tracing::warn!(%error, "failed to configure NumFlow HUD as a non-activating tool window");
    }
}

#[cfg(not(windows))]
fn configure_native_hud_window(_winit_window: &slint::winit_030::winit::window::Window) {}

#[cfg(windows)]
fn position_hud_window(winit_window: &slint::winit_030::winit::window::Window) {
    let size = winit_window.outer_size();
    if let Some(position) =
        numflow_windows::recommended_hud_position(size.width, size.height, HUD_MARGIN_PX)
    {
        winit_window.set_outer_position(slint::winit_030::winit::dpi::PhysicalPosition::new(
            position.x, position.y,
        ));
    }
}

#[cfg(not(windows))]
fn position_hud_window(_winit_window: &slint::winit_030::winit::window::Window) {}

const fn button_headline(button: MouseButton) -> &'static str {
    match button {
        MouseButton::Left => "Left button",
        MouseButton::Right => "Right button",
        MouseButton::Middle => "Middle button",
    }
}

const fn dragging_headline(button: MouseButton) -> &'static str {
    match button {
        MouseButton::Left => "Dragging · Left",
        MouseButton::Right => "Dragging · Right",
        MouseButton::Middle => "Dragging · Middle",
    }
}

const fn button_icon(button: MouseButton) -> HudIconKind {
    match button {
        MouseButton::Left => HudIconKind::Left,
        MouseButton::Right => HudIconKind::Right,
        MouseButton::Middle => HudIconKind::Middle,
    }
}

#[cfg(test)]
mod tests {
    use numflow_core::MouseButton;

    use super::{HudEvent, HudPresentation};

    #[test]
    fn drag_feedback_is_persistent_and_names_the_held_button() {
        let presentation = HudPresentation::from_event(HudEvent::Dragging(MouseButton::Right));

        assert!(presentation.persistent);
        assert_eq!(presentation.headline, "Dragging · Right");
        assert!(presentation.detail.contains("press . to release"));
    }

    #[test]
    fn ordinary_feedback_is_transient() {
        let presentation = HudPresentation::from_event(HudEvent::ButtonSelected(MouseButton::Left));

        assert!(!presentation.persistent);
        assert_eq!(presentation.headline, "Left button");
    }

    #[test]
    fn paused_feedback_is_explicit_without_relying_on_color() {
        let presentation = HudPresentation::from_event(HudEvent::NumFlowEnabled(false));

        assert_eq!(presentation.headline, "NumFlow paused");
        assert_eq!(presentation.detail, "Pointer control disabled");
    }
}
