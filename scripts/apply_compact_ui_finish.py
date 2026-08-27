from pathlib import Path

APP = Path("src/app.rs")
DESIGN = Path("ui/design-system.slint")

text = APP.read_text(encoding="utf-8")

marker = "    fn reset_defaults(&mut self) -> Vec<CoreEffect> {\n"
insert = '''    fn reset_pointer_settings(&mut self) -> Vec<CoreEffect> {
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
        profile.selected_button = default_profile.selected_button;
        self.motion = profile.motion_config();

        let mut effects = self
            .controller
            .apply(InputAction::SelectButton(default_profile.selected_button.into()));
        effects.extend(
            self.controller
                .apply(InputAction::SetPrecision(default_profile.precision_enabled)),
        );
        effects
    }

'''
if "fn reset_pointer_settings" not in text:
    if marker not in text:
        raise SystemExit("reset_defaults marker not found")
    text = text.replace(marker, insert + marker, 1)

sync_marker = '''    window.set_binding_action_index(settings.selected_binding_action_index());
'''
sync_insert = '''    window.set_binding_action_index(settings.selected_binding_action_index());
    window.set_numpad_zero_label(settings.binding_label(NumpadKeyConfig::Num0).into());
'''
if "set_numpad_zero_label" not in text:
    if sync_marker not in text:
        raise SystemExit("binding sync marker not found")
    text = text.replace(sync_marker, sync_insert, 1)

nine_marker = '''    window.set_numpad_nine_label(settings.binding_label(NumpadKeyConfig::Num9).into());
'''
nine_insert = '''    window.set_numpad_nine_label(settings.binding_label(NumpadKeyConfig::Num9).into());
    window.set_numpad_add_label(settings.binding_label(NumpadKeyConfig::Add).into());
    window.set_numpad_decimal_label(settings.binding_label(NumpadKeyConfig::Decimal).into());
'''
if "set_numpad_add_label" not in text:
    if nine_marker not in text:
        raise SystemExit("numpad nine sync marker not found")
    text = text.replace(nine_marker, nine_insert, 1)

prefs_marker = '''    {
        let settings = Rc::clone(settings);
        let hud = Rc::clone(hud);
        let store = Rc::clone(store);
        let runtime = Rc::clone(runtime);
        let weak_window = window.as_weak();
        let weak_tray = tray.as_weak();
        window.on_reset_defaults(move || {
'''
prefs_insert = '''    {
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

'''
if "on_reset_pointer_settings" not in text:
    if prefs_marker not in text:
        raise SystemExit("reset defaults callback marker not found")
    text = text.replace(prefs_marker, prefs_insert + prefs_marker, 1)

APP.write_text(text, encoding="utf-8")

design = DESIGN.read_text(encoding="utf-8")
legacy_start = design.find("export component SectionTitle")
legacy_end = design.find("component SegmentedCell")
if legacy_start != -1:
    if legacy_end == -1 or legacy_end <= legacy_start:
        raise SystemExit("legacy design-system range not found")
    design = design[:legacy_start] + design[legacy_end:]
DESIGN.write_text(design, encoding="utf-8")
