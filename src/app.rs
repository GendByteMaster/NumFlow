use std::{
    cell::RefCell,
    rc::Rc,
    sync::mpsc::Receiver,
    thread::{self, JoinHandle},
};

use num_traits::ToPrimitive;
use numflow_core::{
    Bindings, ControllerState, CoreEffect, InputAction, MotionConfig, MouseButton, StateChange,
};
use slint::ComponentHandle;

use crate::{
    AppTray, AppWindow, MouseButtonMode,
    bindings_ui::{
        action_label, choice_from_index, choice_index, key_from_index, key_index, profile_action,
        reset_profile_bindings, set_profile_binding,
    },
    config::{AppConfig, ConfigError, ConfigLoadStatus, ConfigStore, NumpadKeyConfig},
    error::AppError,
    hud::{HudController, HudEvent},
    runtime::{BackgroundRuntime, RuntimeConfig, RuntimeEvent, RuntimeStateSnapshot, UiSoundCue},
};

const DEFAULT_POINTER_SPEED: f32 = 180.0;
const DEFAULT_POINTER_ACCELERATION: f32 = 900.0;

type SharedUiSettings = Rc<RefCell<UiSettings>>;
type SharedHud = Rc<RefCell<HudController>>;
type SharedConfigStore = Rc<ConfigStore>;
type SharedRuntime = Rc<RefCell<BackgroundRuntime>>;

#[derive(Debug)]
struct UiSettings {
    controller: ControllerState,
    motion: MotionConfig,
    bindings: Bindings,
    config: AppConfig,
    selected_binding_key: NumpadKeyConfig,
}

impl Default for UiSettings {
    fn default() -> Self {
        Self::from_config(AppConfig::default())
    }
}

impl UiSettings {
    fn from_config(config: AppConfig) -> Self {
        let profile = config.active_profile().clone();
        let mut controller = ControllerState::default();
        controller.apply(InputAction::SelectButton(profile.selected_button.into()));
        controller.apply(InputAction::SetPrecision(profile.precision_enabled));

        Self {
            controller,
            motion: profile.motion_config(),
            bindings: profile.bindings(),
            config,
            selected_binding_key: NumpadKeyConfig::Num8,
        }
    }

    fn enabled(&self) -> bool {
        self.controller.is_enabled()
    }

    fn set_enabled(&mut self, enabled: bool) -> Vec<CoreEffect> {
        self.controller.apply(InputAction::SetEnabled(enabled))
    }

    fn shutdown(&mut self) -> Vec<CoreEffect> {
        self.controller.shutdown()
    }

    fn set_mouse_button(&mut self, button: MouseButton) -> Vec<CoreEffect> {
        self.config
            .profiles
            .get_mut(&self.config.active_profile)
            .expect("active profile must exist")
            .selected_button = button.into();
        self.controller.apply(InputAction::SelectButton(button))
    }

    fn set_precision(&mut self, enabled: bool) -> Vec<CoreEffect> {
        self.config
            .profiles
            .get_mut(&self.config.active_profile)
            .expect("active profile must exist")
            .precision_enabled = enabled;
        self.controller.apply(InputAction::SetPrecision(enabled))
    }

    fn set_pointer_speed(&mut self, speed: f32) {
        let mut motion = self.motion;
        motion.base_speed = f64::from(speed);
        self.motion = motion.sanitized();
        self.config
            .profiles
            .get_mut(&self.config.active_profile)
            .expect("active profile must exist")
            .speed = self.motion.base_speed;
    }

    fn set_pointer_acceleration(&mut self, acceleration: f32) {
        let mut motion = self.motion;
        motion.acceleration = f64::from(acceleration);
        self.motion = motion.sanitized();
        self.config
            .profiles
            .get_mut(&self.config.active_profile)
            .expect("active profile must exist")
            .acceleration = self.motion.acceleration;
    }

    fn set_hud_enabled(&mut self, enabled: bool) {
        self.config.hud_enabled = enabled;
    }

    fn hud_enabled(&self) -> bool {
        self.config.hud_enabled
    }

    fn set_sounds_enabled(&mut self, enabled: bool) {
        self.config.sounds_enabled = enabled.into();
    }

    fn sounds_enabled(&self) -> bool {
        self.config.sounds_enabled.get()
    }

    fn set_sound_volume(&mut self, volume_percent: u8) {
        self.config.sound_volume = volume_percent.min(100);
    }

    fn sound_volume(&self) -> u8 {
        self.config.sound_volume.min(100)
    }

    fn set_start_minimized(&mut self, enabled: bool) {
        self.config.start_minimized = enabled;
    }

    fn start_minimized(&self) -> bool {
        self.config.start_minimized
    }

    fn set_start_with_windows(&mut self, enabled: bool) {
        self.config.start_with_windows = enabled;
    }

    fn start_with_windows(&self) -> bool {
        self.config.start_with_windows
    }

    fn set_profile(&mut self, profile_name: &str) -> Result<Vec<CoreEffect>, ConfigError> {
        self.config.set_active_profile(profile_name)?;
        let profile = self.config.active_profile().clone();

        let mut effects = self
            .controller
            .apply(InputAction::SelectButton(profile.selected_button.into()));
        effects.extend(
            self.controller
                .apply(InputAction::SetPrecision(profile.precision_enabled)),
        );
        self.motion = profile.motion_config();
        self.bindings = profile.bindings();

        Ok(effects)
    }

    fn selected_binding_key_index(&self) -> i32 {
        key_index(self.selected_binding_key)
    }

    fn selected_binding_action_index(&self) -> i32 {
        choice_index(profile_action(
            self.config.active_profile(),
            self.selected_binding_key,
        ))
    }

    fn select_binding_key_index(&mut self, index: i32) -> bool {
        let Some(key) = key_from_index(index) else {
            return false;
        };
        if self.selected_binding_key == key {
            return false;
        }

        self.selected_binding_key = key;
        true
    }

    fn set_binding_choice_index(&mut self, index: i32) -> bool {
        let Some(choice) = choice_from_index(index) else {
            return false;
        };
        let profile = self
            .config
            .profiles
            .get_mut(&self.config.active_profile)
            .expect("active profile must exist");
        let changed = set_profile_binding(profile, self.selected_binding_key, choice.action());
        if changed {
            self.bindings = profile.bindings();
        }
        changed
    }

    fn reset_active_bindings(&mut self) -> bool {
        let profile = self
            .config
            .profiles
            .get_mut(&self.config.active_profile)
            .expect("active profile must exist");
        let changed = reset_profile_bindings(profile);
        if changed {
            self.bindings = profile.bindings();
        }
        changed
    }

    fn binding_label(&self, key: NumpadKeyConfig) -> &'static str {
        action_label(profile_action(self.config.active_profile(), key))
    }

    fn held_button(&self) -> Option<MouseButton> {
        self.controller.held_button()
    }

    fn active_profile_name(&self) -> &str {
        &self.config.active_profile
    }

    fn binding_count(&self) -> usize {
        self.bindings.iter().count()
    }

    fn config_snapshot(&self) -> AppConfig {
        self.config.clone()
    }

    fn runtime_config(&self) -> RuntimeConfig {
        RuntimeConfig::new(
            self.motion,
            self.bindings.clone(),
            self.controller.selected_button(),
            self.controller.is_precision_enabled(),
        )
        .with_sounds_enabled(self.sounds_enabled())
        .with_sound_volume(self.sound_volume())
    }

    fn reset_pointer_settings(&mut self) -> Vec<CoreEffect> {
        let defaults = AppConfig::default();
        let default_profile = defaults
            .profiles
            .get(&self.config.active_profile)
            .unwrap_or_else(|| defaults.active_profile())
            .clone();
        let profile = self
            .config
            .profiles
            .get_mut(&self.config.active_profile)
            .expect("active profile must exist");

        profile.speed = default_profile.speed;
        profile.max_speed = default_profile.max_speed;
        profile.acceleration = default_profile.acceleration;
        profile.precision_multiplier = default_profile.precision_multiplier;
        profile.boost_multiplier = default_profile.boost_multiplier;
        profile.precision_enabled = default_profile.precision_enabled;
        self.motion = profile.motion_config();

        self.controller
            .apply(InputAction::SetPrecision(default_profile.precision_enabled))
    }

    fn reset_defaults(&mut self) -> Vec<CoreEffect> {
        self.config = AppConfig::default();
        let profile = self.config.active_profile().clone();

        let mut effects = self
            .controller
            .apply(InputAction::SelectButton(profile.selected_button.into()));
        effects.extend(
            self.controller
                .apply(InputAction::SetPrecision(profile.precision_enabled)),
        );
        self.motion = profile.motion_config();
        self.bindings = profile.bindings();
        self.selected_binding_key = NumpadKeyConfig::Num8;
        effects
    }
}

fn map_mouse_button(button: MouseButtonMode) -> MouseButton {
    match button {
        MouseButtonMode::Left => MouseButton::Left,
        MouseButtonMode::Right => MouseButton::Right,
        MouseButtonMode::Middle => MouseButton::Middle,
    }
}

fn map_mouse_button_to_ui(button: MouseButton) -> MouseButtonMode {
    match button {
        MouseButton::Left => MouseButtonMode::Left,
        MouseButton::Right => MouseButtonMode::Right,
        MouseButton::Middle => MouseButtonMode::Middle,
    }
}

fn ui_float(value: f64, fallback: f32) -> f32 {
    value.to_f32().unwrap_or(fallback)
}

fn sync_binding_view(window: &AppWindow, settings: &UiSettings) {
    window.set_binding_key_index(settings.selected_binding_key_index());
    window.set_binding_action_index(settings.selected_binding_action_index());
    window.set_numpad_zero_label(settings.binding_label(NumpadKeyConfig::Num0).into());
    window.set_numpad_one_label(settings.binding_label(NumpadKeyConfig::Num1).into());
    window.set_numpad_two_label(settings.binding_label(NumpadKeyConfig::Num2).into());
    window.set_numpad_three_label(settings.binding_label(NumpadKeyConfig::Num3).into());
    window.set_numpad_four_label(settings.binding_label(NumpadKeyConfig::Num4).into());
    window.set_numpad_five_label(settings.binding_label(NumpadKeyConfig::Num5).into());
    window.set_numpad_six_label(settings.binding_label(NumpadKeyConfig::Num6).into());
    window.set_numpad_seven_label(settings.binding_label(NumpadKeyConfig::Num7).into());
    window.set_numpad_eight_label(settings.binding_label(NumpadKeyConfig::Num8).into());
    window.set_numpad_nine_label(settings.binding_label(NumpadKeyConfig::Num9).into());
    window.set_numpad_add_label(settings.binding_label(NumpadKeyConfig::Add).into());
    window.set_numpad_decimal_label(settings.binding_label(NumpadKeyConfig::Decimal).into());
}

fn sync_window_from_settings(window: &AppWindow, settings: &UiSettings) {
    window.set_numflow_enabled(settings.enabled());
    window.set_active_button(map_mouse_button_to_ui(
        settings.controller.selected_button(),
    ));
    window.set_precision_enabled(settings.controller.is_precision_enabled());
    window.set_pointer_speed(ui_float(settings.motion.base_speed, DEFAULT_POINTER_SPEED));
    window.set_pointer_acceleration(ui_float(
        settings.motion.acceleration,
        DEFAULT_POINTER_ACCELERATION,
    ));
    window.set_hud_enabled(settings.hud_enabled());
    window.set_sounds_enabled(settings.sounds_enabled());
    window.set_sound_volume(f32::from(settings.sound_volume()));
    window.set_active_profile(settings.active_profile_name().into());
    sync_binding_view(window, settings);
}

fn sync_tray_from_settings(tray: &AppTray, settings: &UiSettings) {
    tray.set_numflow_enabled(settings.enabled());
    tray.set_active_button(map_mouse_button_to_ui(
        settings.controller.selected_button(),
    ));

    if let Some(held_button) = settings.held_button() {
        tray.set_button_held(true);
        tray.set_held_button(map_mouse_button_to_ui(held_button));
    } else {
        tray.set_button_held(false);
        tray.set_held_button(map_mouse_button_to_ui(
            settings.controller.selected_button(),
        ));
    }

    tray.set_start_minimized(settings.start_minimized());
    tray.set_start_with_windows(settings.start_with_windows());
}

fn persist_configuration(settings: &SharedUiSettings, store: &ConfigStore) {
    let config = settings.borrow().config_snapshot();
    if let Err(error) = store.save(&config) {
        tracing::error!(%error, path = %store.path().display(), "failed to save NumFlow configuration");
    }
}

fn runtime_apply(runtime: &SharedRuntime, action: InputAction) {
    if let Err(error) = runtime.borrow().apply(action) {
        tracing::error!(%error, ?action, "failed to send action to NumFlow background runtime");
    }
}

fn runtime_set_enabled(runtime: &SharedRuntime, enabled: bool) -> bool {
    match runtime.borrow().set_enabled(enabled) {
        Ok(()) => true,
        Err(error) => {
            tracing::error!(
                %error,
                enabled,
                "failed to send enabled mode to NumFlow background runtime"
            );
            false
        }
    }
}

fn runtime_configure(runtime: &SharedRuntime, settings: &SharedUiSettings) {
    let config = settings.borrow().runtime_config();
    if let Err(error) = runtime.borrow().configure(config) {
        tracing::error!(%error, "failed to configure NumFlow background runtime");
    }
}

fn runtime_set_motion(runtime: &SharedRuntime, settings: &SharedUiSettings) {
    let motion = settings.borrow().motion;
    if let Err(error) = runtime.borrow().set_motion_config(motion) {
        tracing::error!(%error, "failed to update background pointer motion config");
    }
}

fn runtime_set_bindings(runtime: &SharedRuntime, settings: &SharedUiSettings) {
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

fn runtime_set_sound_volume(runtime: &SharedRuntime, volume_percent: u8) {
    if let Err(error) = runtime.borrow().set_sound_volume(volume_percent) {
        tracing::warn!(
            %error,
            volume_percent,
            "failed to update interface sound volume in runtime"
        );
    }
}

#[cfg(windows)]
fn set_windows_startup(enabled: bool) -> bool {
    match numflow_windows::StartupRegistration::set_enabled(enabled) {
        Ok(()) => true,
        Err(error) => {
            tracing::error!(%error, enabled, "failed to update Windows startup registration");
            false
        }
    }
}

#[cfg(not(windows))]
fn set_windows_startup(_enabled: bool) -> bool {
    true
}

#[cfg(windows)]
fn configure_main_window_material(window: &AppWindow) {
    use slint::winit_030::{
        WinitWindowAccessor,
        winit::platform::windows::{BackdropType, WindowExtWindows},
    };

    let weak_window = window.as_weak();
    slint::Timer::single_shot(std::time::Duration::ZERO, move || {
        let Some(window) = weak_window.upgrade() else {
            return;
        };

        let configured = window.window().with_winit_window(|winit_window| {
            // Mica is a native Windows 11 system backdrop. Unsupported systems simply keep the
            // translucent Slint material fallback drawn by the UI.
            winit_window.set_system_backdrop(BackdropType::MainWindow);
        });

        if configured.is_none() {
            tracing::warn!(
                "NumFlow glass material requires the Slint winit backend; using translucent fallback"
            );
        }
    });
}

#[cfg(not(windows))]
fn configure_main_window_material(_window: &AppWindow) {}

fn connect_pointer_controls(
    window: &AppWindow,
    tray: &AppTray,
    settings: &SharedUiSettings,
    hud: &SharedHud,
    store: &SharedConfigStore,
    runtime: &SharedRuntime,
) {
    {
        let settings = Rc::clone(settings);
        let hud = Rc::clone(hud);
        let runtime = Rc::clone(runtime);
        let weak_window = window.as_weak();
        let weak_tray = tray.as_weak();
        window.on_enabled_toggled(move |enabled| {
            if !runtime_set_enabled(&runtime, enabled) {
                let previous = settings.borrow().enabled();
                if let Some(window) = weak_window.upgrade() {
                    window.set_numflow_enabled(previous);
                }
                if let Some(tray) = weak_tray.upgrade() {
                    tray.set_numflow_enabled(previous);
                }
                return;
            }

            let effects = settings.borrow_mut().set_enabled(enabled);
            hud.borrow_mut().observe_effects(&effects);
            if let Some(tray) = weak_tray.upgrade() {
                tray.set_numflow_enabled(enabled);
            }
        });
    }

    {
        let settings = Rc::clone(settings);
        let hud = Rc::clone(hud);
        let store = Rc::clone(store);
        let runtime = Rc::clone(runtime);
        window.on_mouse_button_changed(move |button| {
            let button = map_mouse_button(button);
            let effects = settings.borrow_mut().set_mouse_button(button);
            runtime_apply(&runtime, InputAction::SelectButton(button));
            hud.borrow_mut().observe_effects(&effects);
            persist_configuration(&settings, &store);
        });
    }

    {
        let settings = Rc::clone(settings);
        let hud = Rc::clone(hud);
        let store = Rc::clone(store);
        let runtime = Rc::clone(runtime);
        window.on_precision_toggled(move |enabled| {
            let effects = settings.borrow_mut().set_precision(enabled);
            runtime_apply(&runtime, InputAction::SetPrecision(enabled));
            hud.borrow_mut().observe_effects(&effects);
            persist_configuration(&settings, &store);
        });
    }

    {
        let settings = Rc::clone(settings);
        let store = Rc::clone(store);
        let runtime = Rc::clone(runtime);
        window.on_speed_changed(move |speed| {
            settings.borrow_mut().set_pointer_speed(speed);
            runtime_set_motion(&runtime, &settings);
            persist_configuration(&settings, &store);
        });
    }

    {
        let settings = Rc::clone(settings);
        let store = Rc::clone(store);
        let runtime = Rc::clone(runtime);
        window.on_acceleration_changed(move |acceleration| {
            settings.borrow_mut().set_pointer_acceleration(acceleration);
            runtime_set_motion(&runtime, &settings);
            persist_configuration(&settings, &store);
        });
    }
}

fn connect_binding_controls(
    window: &AppWindow,
    settings: &SharedUiSettings,
    store: &SharedConfigStore,
    runtime: &SharedRuntime,
) {
    {
        let settings = Rc::clone(settings);
        let weak_window = window.as_weak();
        window.on_binding_key_changed(move |index| {
            if !settings.borrow_mut().select_binding_key_index(index) {
                return;
            }
            if let Some(window) = weak_window.upgrade() {
                window.set_binding_action_index(settings.borrow().selected_binding_action_index());
            }
        });
    }

    {
        let settings = Rc::clone(settings);
        let store = Rc::clone(store);
        let runtime = Rc::clone(runtime);
        let weak_window = window.as_weak();
        window.on_binding_action_changed(move |index| {
            if !settings.borrow_mut().set_binding_choice_index(index) {
                return;
            }
            runtime_set_bindings(&runtime, &settings);
            if let Some(window) = weak_window.upgrade() {
                sync_binding_view(&window, &settings.borrow());
            }
            persist_configuration(&settings, &store);
        });
    }

    {
        let settings = Rc::clone(settings);
        let store = Rc::clone(store);
        let runtime = Rc::clone(runtime);
        let weak_window = window.as_weak();
        window.on_reset_bindings(move || {
            if !settings.borrow_mut().reset_active_bindings() {
                return;
            }
            runtime_set_bindings(&runtime, &settings);
            if let Some(window) = weak_window.upgrade() {
                sync_binding_view(&window, &settings.borrow());
            }
            persist_configuration(&settings, &store);
        });
    }
}

fn connect_sound_preferences(
    window: &AppWindow,
    settings: &SharedUiSettings,
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
        let store = Rc::clone(store);
        let runtime = Rc::clone(runtime);
        window.on_sound_volume_changed(move |volume| {
            let volume_percent = volume.round().to_u8().unwrap_or(25).min(100);
            if settings.borrow().sound_volume() == volume_percent {
                return;
            }
            settings.borrow_mut().set_sound_volume(volume_percent);
            runtime_set_sound_volume(&runtime, volume_percent);
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
}

fn connect_preferences(
    window: &AppWindow,
    tray: &AppTray,
    settings: &SharedUiSettings,
    hud: &SharedHud,
    store: &SharedConfigStore,
    runtime: &SharedRuntime,
) {
    {
        let settings = Rc::clone(settings);
        let hud = Rc::clone(hud);
        let store = Rc::clone(store);
        window.on_hud_toggled(move |enabled| {
            let held_button = {
                let mut settings = settings.borrow_mut();
                settings.set_hud_enabled(enabled);
                settings.held_button()
            };

            let mut hud = hud.borrow_mut();
            hud.set_enabled(enabled);
            if enabled {
                hud.sync_held_button(held_button);
                hud.show_event(HudEvent::HudEnabled);
            }
            drop(hud);
            persist_configuration(&settings, &store);
        });
    }

    {
        let settings = Rc::clone(settings);
        let hud = Rc::clone(hud);
        let store = Rc::clone(store);
        let runtime = Rc::clone(runtime);
        let weak_window = window.as_weak();
        window.on_profile_changed(move |profile| {
            let effects = match settings.borrow_mut().set_profile(profile.as_str()) {
                Ok(effects) => effects,
                Err(error) => {
                    tracing::error!(%error, profile = %profile, "failed to activate profile");
                    return;
                }
            };

            runtime_configure(&runtime, &settings);
            hud.borrow_mut().observe_effects(&effects);
            if let Some(window) = weak_window.upgrade() {
                sync_window_from_settings(&window, &settings.borrow());
            }
            persist_configuration(&settings, &store);
        });
    }

    {
        let settings = Rc::clone(settings);
        let hud = Rc::clone(hud);
        let store = Rc::clone(store);
        let runtime = Rc::clone(runtime);
        let weak_window = window.as_weak();
        let weak_tray = tray.as_weak();
        window.on_reset_pointer_settings(move || {
            let effects = settings.borrow_mut().reset_pointer_settings();
            runtime_configure(&runtime, &settings);
            hud.borrow_mut().observe_effects(&effects);

            if let Some(window) = weak_window.upgrade() {
                sync_window_from_settings(&window, &settings.borrow());
            }
            if let Some(tray) = weak_tray.upgrade() {
                sync_tray_from_settings(&tray, &settings.borrow());
            }
            persist_configuration(&settings, &store);
        });
    }

    {
        let settings = Rc::clone(settings);
        let hud = Rc::clone(hud);
        let store = Rc::clone(store);
        let runtime = Rc::clone(runtime);
        let weak_window = window.as_weak();
        let weak_tray = tray.as_weak();
        window.on_reset_defaults(move || {
            let (effects, held_button) = {
                let mut settings = settings.borrow_mut();
                let effects = settings.reset_defaults();
                (effects, settings.held_button())
            };

            if !set_windows_startup(false) {
                settings.borrow_mut().set_start_with_windows(true);
            }
            runtime_configure(&runtime, &settings);

            {
                let mut hud = hud.borrow_mut();
                hud.set_enabled(true);
                hud.observe_effects(&effects);
                hud.sync_held_button(held_button);
                hud.show_event(HudEvent::DefaultsRestored);
            }

            if let Some(window) = weak_window.upgrade() {
                sync_window_from_settings(&window, &settings.borrow());
            }
            if let Some(tray) = weak_tray.upgrade() {
                sync_tray_from_settings(&tray, &settings.borrow());
            }
            persist_configuration(&settings, &store);
        });
    }
}

fn connect_tray(
    window: &AppWindow,
    tray: &AppTray,
    settings: &SharedUiSettings,
    hud: &SharedHud,
    store: &SharedConfigStore,
    runtime: &SharedRuntime,
) {
    {
        let weak_window = window.as_weak();
        tray.on_open_settings(move || {
            if let Some(window) = weak_window.upgrade()
                && let Err(error) = window.show()
            {
                tracing::error!(%error, "failed to show NumFlow settings window");
            }
        });
    }

    {
        let settings = Rc::clone(settings);
        let hud = Rc::clone(hud);
        let runtime = Rc::clone(runtime);
        let weak_window = window.as_weak();
        let weak_tray = tray.as_weak();
        tray.on_enabled_toggled(move |enabled| {
            if !runtime_set_enabled(&runtime, enabled) {
                let previous = settings.borrow().enabled();
                if let Some(window) = weak_window.upgrade() {
                    window.set_numflow_enabled(previous);
                }
                if let Some(tray) = weak_tray.upgrade() {
                    tray.set_numflow_enabled(previous);
                }
                return;
            }

            let effects = settings.borrow_mut().set_enabled(enabled);
            hud.borrow_mut().observe_effects(&effects);
            if let Some(window) = weak_window.upgrade() {
                window.set_numflow_enabled(enabled);
            }
            if let Some(tray) = weak_tray.upgrade() {
                tray.set_numflow_enabled(enabled);
            }
        });
    }

    {
        let settings = Rc::clone(settings);
        let store = Rc::clone(store);
        let weak_tray = tray.as_weak();
        tray.on_start_minimized_toggled(move |enabled| {
            settings.borrow_mut().set_start_minimized(enabled);
            if let Some(tray) = weak_tray.upgrade() {
                tray.set_start_minimized(enabled);
            }
            persist_configuration(&settings, &store);
        });
    }

    {
        let settings = Rc::clone(settings);
        let store = Rc::clone(store);
        let weak_tray = tray.as_weak();
        tray.on_start_with_windows_toggled(move |enabled| {
            let previous = settings.borrow().start_with_windows();
            if !set_windows_startup(enabled) {
                if let Some(tray) = weak_tray.upgrade() {
                    tray.set_start_with_windows(previous);
                }
                return;
            }

            settings.borrow_mut().set_start_with_windows(enabled);
            if let Some(tray) = weak_tray.upgrade() {
                tray.set_start_with_windows(enabled);
            }
            persist_configuration(&settings, &store);
        });
    }

    {
        let settings = Rc::clone(settings);
        let hud = Rc::clone(hud);
        let runtime = Rc::clone(runtime);
        tray.on_exit_requested(move || {
            if let Err(error) = runtime.borrow_mut().shutdown() {
                tracing::error!(%error, "background runtime failed during shutdown");
            }
            let effects = settings.borrow_mut().shutdown();
            hud.borrow_mut().observe_effects(&effects);
            tracing::info!("exit requested from NumFlow system tray");
            if let Err(error) = slint::quit_event_loop() {
                tracing::error!(%error, "failed to request NumFlow event-loop shutdown");
            }
        });
    }
}

fn sync_runtime_state(settings: &mut UiSettings, state: RuntimeStateSnapshot) -> bool {
    let selected_button = state.selected_button.into();
    let profile = settings
        .config
        .profiles
        .get_mut(&settings.config.active_profile)
        .expect("active profile must exist");
    let config_changed =
        profile.selected_button != selected_button || profile.precision_enabled != state.precision;
    profile.selected_button = selected_button;
    profile.precision_enabled = state.precision;

    // Rebuild the UI-side state machine from the authoritative runtime snapshot. This keeps
    // settings/tray/HUD state correct even if an older transient event was evicted under load.
    let _ = settings.controller.apply(InputAction::SetEnabled(false));
    let _ = settings
        .controller
        .apply(InputAction::SelectButton(state.selected_button));
    let _ = settings
        .controller
        .apply(InputAction::SetPrecision(state.precision));

    if state.enabled {
        let _ = settings.controller.apply(InputAction::SetEnabled(true));
        if let Some(held_button) = state.held_button {
            let _ = settings
                .controller
                .apply(InputAction::SelectButton(held_button));
            let _ = settings.controller.apply(InputAction::Hold);
            let _ = settings
                .controller
                .apply(InputAction::SelectButton(state.selected_button));
        }
    }

    config_changed
}

fn start_runtime_event_bridge(
    window: &AppWindow,
    tray: &AppTray,
    settings: &SharedUiSettings,
    hud: &SharedHud,
    store: &SharedConfigStore,
    runtime: &SharedRuntime,
    wake_receiver: Option<Receiver<()>>,
) -> Result<Option<JoinHandle<()>>, AppError> {
    let weak_window = window.as_weak();
    let weak_tray = tray.as_weak();
    let settings = Rc::clone(settings);
    let hud = Rc::clone(hud);
    let store = Rc::clone(store);
    let runtime = Rc::clone(runtime);

    window.on_runtime_events_ready(move || {
        let events = runtime.borrow().drain_events();
        if events.is_empty() {
            return;
        }

        let mut state_changed = false;
        let mut config_changed = false;
        for event in events {
            match event {
                RuntimeEvent::Effects { state, effects } => {
                    config_changed |= sync_runtime_state(&mut settings.borrow_mut(), state);
                    hud.borrow_mut().observe_effects(&effects);
                    state_changed = true;
                }
                RuntimeEvent::Fault { state, reason } => {
                    config_changed |= sync_runtime_state(&mut settings.borrow_mut(), state);
                    tracing::error!(%reason, "NumFlow background pointer runtime entered safe disabled state");
                    runtime_play_sound(&runtime, UiSoundCue::Error);
                    hud.borrow_mut().observe_effects(&[CoreEffect::State(
                        StateChange::Enabled(false),
                    )]);
                    state_changed = true;
                }
            }
        }

        if state_changed {
            if let Some(window) = weak_window.upgrade() {
                sync_window_from_settings(&window, &settings.borrow());
            }
            if let Some(tray) = weak_tray.upgrade() {
                sync_tray_from_settings(&tray, &settings.borrow());
            }
        }
        if config_changed {
            persist_configuration(&settings, &store);
        }
    });

    let Some(wake_receiver) = wake_receiver else {
        return Ok(None);
    };
    let weak_window = window.as_weak();
    let join = thread::Builder::new()
        .name("numflow-runtime-events".to_owned())
        .spawn(move || {
            while wake_receiver.recv().is_ok() {
                if weak_window
                    .upgrade_in_event_loop(|window| window.invoke_runtime_events_ready())
                    .is_err()
                {
                    break;
                }
            }
        })
        .map_err(|error| {
            AppError::Runtime(format!("failed to start runtime event bridge: {error}"))
        })?;

    Ok(Some(join))
}

fn connect_ui(
    window: &AppWindow,
    tray: &AppTray,
    settings: &SharedUiSettings,
    hud: &SharedHud,
    store: &SharedConfigStore,
    runtime: &SharedRuntime,
) {
    connect_pointer_controls(window, tray, settings, hud, store, runtime);
    connect_binding_controls(window, settings, store, runtime);
    connect_sound_preferences(window, settings, store, runtime);
    connect_preferences(window, tray, settings, hud, store, runtime);
    connect_tray(window, tray, settings, hud, store, runtime);
}

fn show_main_window_if_needed(
    window: &AppWindow,
    settings: &UiSettings,
    background: bool,
) -> Result<(), AppError> {
    if background || settings.start_minimized() {
        tracing::info!(
            background,
            start_minimized = settings.start_minimized(),
            "starting NumFlow with settings window hidden"
        );
        Ok(())
    } else {
        window
            .show()
            .map_err(|error| AppError::Ui(error.to_string()))
    }
}

pub fn run(background: bool) -> Result<(), AppError> {
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "starting NumFlow");

    let store = Rc::new(ConfigStore::for_current_user()?);
    let loaded = store.load_or_default()?;
    let should_write_defaults = match &loaded.status {
        ConfigLoadStatus::Loaded => false,
        ConfigLoadStatus::Missing => {
            tracing::info!(path = %store.path().display(), "configuration missing; using safe defaults");
            true
        }
        ConfigLoadStatus::Recovered { reason } => {
            tracing::warn!(%reason, path = %store.path().display(), "invalid configuration recovered to safe defaults");
            true
        }
    };

    if should_write_defaults {
        store.save(&loaded.config)?;
    }

    let settings = Rc::new(RefCell::new(UiSettings::from_config(loaded.config)));

    // Install the low-level keyboard hook and apply the current Num Lock mode before creating
    // any visible NumFlow UI. Once the tray icon appears, keyboard interception is already ready.
    let mut background_runtime = BackgroundRuntime::start(settings.borrow().runtime_config())
        .map_err(|error| AppError::Runtime(error.to_string()))?;
    let runtime_wake_receiver = background_runtime.take_wake_receiver();
    let runtime = Rc::new(RefCell::new(background_runtime));

    let tray = AppTray::new().map_err(|error| AppError::Ui(error.to_string()))?;
    tracing::info!("NumFlow system tray ready; keyboard runtime is already active");
    let window = AppWindow::new().map_err(|error| AppError::Ui(error.to_string()))?;
    configure_main_window_material(&window);

    crate::platform_input::prepare_after_ui().map_err(AppError::Runtime)?;

    #[cfg(windows)]
    let reduced_motion = match numflow_windows::client_area_animations_enabled() {
        Ok(enabled) => !enabled,
        Err(error) => {
            tracing::warn!(
                %error,
                "failed to read Windows client-area animation preference; using standard UI motion"
            );
            false
        }
    };
    #[cfg(not(windows))]
    let reduced_motion = false;
    window.set_reduced_motion(reduced_motion);

    if let Err(error) = runtime.borrow().resync_input_state() {
        tracing::warn!(%error, "failed to queue startup input runtime resynchronization");
    }

    sync_window_from_settings(&window, &settings.borrow());
    sync_tray_from_settings(&tray, &settings.borrow());

    if !set_windows_startup(settings.borrow().start_with_windows()) {
        tracing::warn!("configured Windows startup preference could not be applied");
    }

    let hud = Rc::new(RefCell::new(
        HudController::new().map_err(|error| AppError::Ui(error.to_string()))?,
    ));
    hud.borrow_mut()
        .set_enabled(settings.borrow().hud_enabled());
    hud.borrow_mut().set_reduced_motion(reduced_motion);

    tracing::info!(
        profile = settings.borrow().active_profile_name(),
        bindings = settings.borrow().binding_count(),
        start_minimized = settings.borrow().start_minimized(),
        start_with_windows = settings.borrow().start_with_windows(),
        sounds_enabled = settings.borrow().sounds_enabled(),
        path = %store.path().display(),
        "configuration and background runtime ready"
    );

    connect_ui(&window, &tray, &settings, &hud, &store, &runtime);
    let runtime_event_bridge = start_runtime_event_bridge(
        &window,
        &tray,
        &settings,
        &hud,
        &store,
        &runtime,
        runtime_wake_receiver,
    )?;

    show_main_window_if_needed(&window, &settings.borrow(), background)?;

    let event_loop_result =
        slint::run_event_loop().map_err(|error| AppError::Ui(error.to_string()));
    if let Err(error) = runtime.borrow_mut().shutdown() {
        tracing::error!(%error, "background runtime failed during final shutdown");
    }
    if let Some(join) = runtime_event_bridge
        && join.join().is_err()
    {
        tracing::error!("runtime event bridge thread panicked during shutdown");
    }
    event_loop_result
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_POINTER_ACCELERATION, DEFAULT_POINTER_SPEED, UiSettings, sync_runtime_state,
    };
    use crate::runtime::RuntimeStateSnapshot;
    use crate::{
        bindings_ui::choice_index,
        config::{AppConfig, InputActionConfig},
    };
    use numflow_core::{Direction, InputAction, MotionConfig, MouseButton, NumpadKey};

    #[test]
    fn runtime_snapshot_resynchronizes_ui_state_after_event_coalescing() {
        let mut settings = UiSettings::default();
        let changed = sync_runtime_state(
            &mut settings,
            RuntimeStateSnapshot {
                enabled: true,
                selected_button: MouseButton::Right,
                held_button: Some(MouseButton::Left),
                precision: true,
            },
        );

        assert!(changed);
        assert!(settings.controller.is_enabled());
        assert_eq!(settings.controller.selected_button(), MouseButton::Right);
        assert_eq!(settings.controller.held_button(), Some(MouseButton::Left));
        assert!(settings.controller.is_precision_enabled());
        assert_eq!(
            MouseButton::from(settings.config.active_profile().selected_button),
            MouseButton::Right
        );
        assert!(settings.config.active_profile().precision_enabled);
    }

    #[test]
    fn ui_defaults_match_core_motion_defaults() {
        let defaults = MotionConfig::default();

        assert!((defaults.base_speed - f64::from(DEFAULT_POINTER_SPEED)).abs() <= f64::EPSILON);
        assert!(
            (defaults.acceleration - f64::from(DEFAULT_POINTER_ACCELERATION)).abs() <= f64::EPSILON
        );
    }

    #[test]
    fn pointer_controls_update_profile_and_motion_config() {
        let mut settings = UiSettings::default();

        settings.set_pointer_speed(420.0);
        settings.set_pointer_acceleration(1_600.0);

        assert!((settings.motion.base_speed - 420.0).abs() <= f64::EPSILON);
        assert!((settings.motion.acceleration - 1_600.0).abs() <= f64::EPSILON);
        assert!((settings.config.active_profile().speed - 420.0).abs() <= f64::EPSILON);
        assert!((settings.config.active_profile().acceleration - 1_600.0).abs() <= f64::EPSILON);
    }

    #[test]
    fn reset_pointer_settings_preserves_selected_mouse_button() {
        let mut settings = UiSettings::default();
        settings.set_mouse_button(MouseButton::Right);
        settings.set_pointer_speed(640.0);
        settings.set_pointer_acceleration(2_400.0);
        settings.set_precision(true);

        let _ = settings.reset_pointer_settings();

        assert_eq!(settings.controller.selected_button(), MouseButton::Right);
        assert!(
            (settings.motion.base_speed - f64::from(DEFAULT_POINTER_SPEED)).abs() <= f64::EPSILON
        );
        assert!(
            (settings.motion.acceleration - f64::from(DEFAULT_POINTER_ACCELERATION)).abs()
                <= f64::EPSILON
        );
        assert!(!settings.controller.is_precision_enabled());
    }

    #[test]
    fn hud_feedback_is_enabled_by_default() {
        assert!(UiSettings::default().hud_enabled());
    }

    #[test]
    fn lifecycle_preferences_are_disabled_by_default() {
        let settings = UiSettings::default();

        assert!(!settings.start_minimized());
        assert!(!settings.start_with_windows());
    }

    #[test]
    fn lifecycle_preferences_update_typed_config() {
        let mut settings = UiSettings::default();

        settings.set_start_minimized(true);
        settings.set_start_with_windows(true);

        assert!(settings.start_minimized());
        assert!(settings.start_with_windows());
        assert!(settings.config.start_minimized);
        assert!(settings.config.start_with_windows);
    }

    #[test]
    fn runtime_config_matches_active_ui_state() {
        let mut settings = UiSettings::default();
        settings.set_mouse_button(MouseButton::Right);
        settings.set_precision(true);
        settings.set_pointer_speed(420.0);

        let runtime = settings.runtime_config();

        assert_eq!(runtime.selected_button, MouseButton::Right);
        assert!(runtime.precision);
        assert!((runtime.motion.base_speed - 420.0).abs() <= f64::EPSILON);
        assert_eq!(runtime.bindings.iter().count(), settings.binding_count());
    }

    #[test]
    fn persisted_profile_is_applied_to_runtime_state() {
        let config = AppConfig {
            active_profile: "Precision".to_owned(),
            hud_enabled: false,
            ..AppConfig::default()
        };
        let settings = UiSettings::from_config(config);

        assert_eq!(settings.active_profile_name(), "Precision");
        assert!(settings.controller.is_precision_enabled());
        assert!(!settings.hud_enabled());
        assert!((settings.motion.base_speed - 130.0).abs() <= f64::EPSILON);
        assert_eq!(settings.binding_count(), 15);
    }

    #[test]
    fn profile_switch_updates_runtime_controls() {
        let mut settings = UiSettings::default();

        settings
            .set_profile("Fast")
            .expect("built-in Fast profile should exist");

        assert_eq!(settings.active_profile_name(), "Fast");
        assert!((settings.motion.base_speed - 300.0).abs() <= f64::EPSILON);
        assert!((settings.motion.acceleration - 1_600.0).abs() <= f64::EPSILON);
    }

    #[test]
    fn custom_binding_updates_runtime_and_active_profile() {
        let mut settings = UiSettings::default();
        let click_index = choice_index(Some(InputActionConfig::Click));

        assert!(settings.set_binding_choice_index(click_index));
        assert_eq!(
            settings.bindings.action_for(NumpadKey::Num8),
            Some(InputAction::Click)
        );
        assert_eq!(
            settings.binding_label(crate::config::NumpadKeyConfig::Num8),
            "Click"
        );
    }

    #[test]
    fn custom_binding_is_profile_specific_and_can_be_unbound() {
        let mut settings = UiSettings::default();
        settings.set_binding_choice_index(choice_index(Some(InputActionConfig::Click)));

        settings
            .set_profile("Fast")
            .expect("built-in Fast profile should exist");
        assert_eq!(
            settings.bindings.action_for(NumpadKey::Num8),
            Some(InputAction::Move(Direction::Up))
        );

        settings
            .set_profile("Normal")
            .expect("built-in Normal profile should exist");
        assert!(settings.set_binding_choice_index(choice_index(None)));
        assert_eq!(settings.bindings.action_for(NumpadKey::Num8), None);
        assert_eq!(settings.binding_count(), 14);
    }

    #[test]
    fn reset_active_bindings_restores_default_mapping() {
        let mut settings = UiSettings::default();
        settings.set_binding_choice_index(choice_index(None));

        assert!(settings.reset_active_bindings());
        assert_eq!(
            settings.bindings.action_for(NumpadKey::Num8),
            Some(InputAction::Move(Direction::Up))
        );
        assert_eq!(settings.binding_count(), 15);
        assert!(!settings.reset_active_bindings());
    }

    #[test]
    fn reset_restores_settings_without_disabling_runtime_state() {
        let mut settings = UiSettings::default();
        settings.set_enabled(true);
        settings.set_mouse_button(MouseButton::Right);
        settings.set_precision(true);
        settings.set_pointer_speed(520.0);
        settings.set_pointer_acceleration(2_000.0);
        settings.set_hud_enabled(false);
        settings.set_start_minimized(true);
        settings.set_start_with_windows(true);
        settings
            .set_profile("Fast")
            .expect("built-in Fast profile should exist");

        settings.reset_defaults();

        assert!(settings.controller.is_enabled());
        assert_eq!(settings.controller.selected_button(), MouseButton::Left);
        assert!(!settings.controller.is_precision_enabled());
        assert!((settings.motion.base_speed - 180.0).abs() <= f64::EPSILON);
        assert!((settings.motion.acceleration - 900.0).abs() <= f64::EPSILON);
        assert!(settings.hud_enabled());
        assert_eq!(settings.active_profile_name(), "Normal");
        assert!(!settings.start_minimized());
        assert!(!settings.start_with_windows());
    }

    #[test]
    fn reset_does_not_drop_an_active_drag() {
        let mut settings = UiSettings::default();
        settings.set_enabled(true);
        settings.controller.apply(InputAction::Hold);

        settings.reset_defaults();

        assert_eq!(settings.controller.held_button(), Some(MouseButton::Left));
    }

    #[test]
    fn shutdown_disables_controller_and_releases_drag_state() {
        let mut settings = UiSettings::default();
        settings.set_enabled(true);
        settings.controller.apply(InputAction::Hold);

        let effects = settings.shutdown();

        assert!(!settings.enabled());
        assert_eq!(settings.controller.held_button(), None);
        assert!(!effects.is_empty());
    }
}
