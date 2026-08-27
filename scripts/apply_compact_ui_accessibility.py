from pathlib import Path

APP = Path("src/app.rs")
LIB = Path("crates/numflow-windows/src/lib.rs")
ACCESSIBILITY = Path("crates/numflow-windows/src/accessibility.rs")
UI = Path("ui/app.slint")

ACCESSIBILITY.write_text(
    '''use std::ffi::c_void;\n\nuse windows::{\n    Win32::{\n        Foundation::BOOL,\n        UI::WindowsAndMessaging::{\n            SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,\n            SystemParametersInfoW,\n        },\n    },\n    core::Result,\n};\n\n/// Returns whether Windows client-area animations are enabled for the current user.\n///\n/// # Errors\n///\n/// Returns the underlying Win32 error if the accessibility preference cannot be queried.\npub fn client_area_animations_enabled() -> Result<bool> {\n    let mut enabled = BOOL::from(true);\n    // SAFETY: `enabled` is a live BOOL for the duration of the call and the selected SPI action\n    // requires `pvParam` to point to a writable BOOL. No pointer escapes this function.\n    unsafe {\n        SystemParametersInfoW(\n            SPI_GETCLIENTAREAANIMATION,\n            0,\n            Some(std::ptr::addr_of_mut!(enabled).cast::<c_void>()),\n            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),\n        )?;\n    }\n    Ok(enabled.as_bool())\n}\n''',
    encoding="utf-8",
)

lib = LIB.read_text(encoding="utf-8")
if "mod accessibility;" not in lib:
    lib = lib.replace("#[cfg(windows)]\nmod audio;", "#[cfg(windows)]\nmod accessibility;\n#[cfg(windows)]\nmod audio;", 1)
if "client_area_animations_enabled" not in lib:
    export_marker = "#[cfg(windows)]\npub use audio::{AudioCue, AudioFeedbackError, AudioFeedbackService};"
    lib = lib.replace(
        export_marker,
        "#[cfg(windows)]\npub use accessibility::client_area_animations_enabled;\n" + export_marker,
        1,
    )
LIB.write_text(lib, encoding="utf-8")

app = APP.read_text(encoding="utf-8")
old_reset = '''        profile.boost_multiplier = default_profile.boost_multiplier;
        profile.precision_enabled = default_profile.precision_enabled;
        profile.selected_button = default_profile.selected_button;
        self.motion = profile.motion_config();

        let mut effects = self.controller.apply(InputAction::SelectButton(
            default_profile.selected_button.into(),
        ));
        effects.extend(
            self.controller
                .apply(InputAction::SetPrecision(default_profile.precision_enabled)),
        );
        effects
'''
new_reset = '''        profile.boost_multiplier = default_profile.boost_multiplier;
        profile.precision_enabled = default_profile.precision_enabled;
        self.motion = profile.motion_config();

        self.controller
            .apply(InputAction::SetPrecision(default_profile.precision_enabled))
'''
if old_reset not in app:
    raise SystemExit("reset_pointer_settings body marker not found")
app = app.replace(old_reset, new_reset, 1)

window_marker = '''    let window = AppWindow::new().map_err(|error| AppError::Ui(error.to_string()))?;

    #[cfg(windows)]
    if let Err(error) = numflow_windows::remove_raw_keyboard_device_event_registration() {
'''
window_insert = '''    let window = AppWindow::new().map_err(|error| AppError::Ui(error.to_string()))?;

    #[cfg(windows)]
    match numflow_windows::client_area_animations_enabled() {
        Ok(enabled) => window.set_reduced_motion(!enabled),
        Err(error) => tracing::warn!(
            %error,
            "failed to read Windows client-area animation preference; using standard UI motion"
        ),
    }

    #[cfg(windows)]
    if let Err(error) = numflow_windows::remove_raw_keyboard_device_event_registration() {
'''
if "client_area_animations_enabled()" not in app:
    if window_marker not in app:
        raise SystemExit("window creation marker not found")
    app = app.replace(window_marker, window_insert, 1)

# Ensure Reset pointer settings does not unexpectedly change the selected mouse button.
test_marker = '''    #[test]
    fn hud_feedback_is_enabled_by_default() {
'''
test_insert = '''    #[test]
    fn reset_pointer_settings_preserves_selected_mouse_button() {
        let mut settings = UiSettings::default();
        settings.set_mouse_button(MouseButton::Right);
        settings.set_pointer_speed(640.0);
        settings.set_pointer_acceleration(2_400.0);
        settings.set_precision(true);

        let _ = settings.reset_pointer_settings();

        assert_eq!(settings.controller.selected_button(), MouseButton::Right);
        assert!((settings.motion.base_speed - f64::from(DEFAULT_POINTER_SPEED)).abs() <= f64::EPSILON);
        assert!(
            (settings.motion.acceleration - f64::from(DEFAULT_POINTER_ACCELERATION)).abs()
                <= f64::EPSILON
        );
        assert!(!settings.controller.is_precision_enabled());
    }

'''
if "reset_pointer_settings_preserves_selected_mouse_button" not in app:
    if test_marker not in app:
        raise SystemExit("test insertion marker not found")
    app = app.replace(test_marker, test_insert + test_marker, 1)
APP.write_text(app, encoding="utf-8")

ui = UI.read_text(encoding="utf-8")
ui = ui.replace("preferred-height: 410px;", "preferred-height: 420px;", 1)
ui = ui.replace("min-height: 360px;", "min-height: 390px;", 1)

function_marker = '''    function mark-saved() {
'''
function_insert = '''    function binding-glyph(label: string) -> string {
        return label == "Up-left" ? "↖" :
            label == "Up" ? "↑" :
            label == "Up-right" ? "↗" :
            label == "Left" ? "←" :
            label == "Right" ? "→" :
            label == "Down-left" ? "↙" :
            label == "Down" ? "↓" :
            label == "Down-right" ? "↘" :
            label == "Click" || label == "Single click" ? "•" :
            label == "Double click" ? "••" :
            label == "Hold / drag" ? "⌁" :
            label == "Release" ? "↥" : "";
    }

    function binding-display(label: string) -> string {
        let glyph = root.binding-glyph(label);
        return glyph == "" ? label : glyph + "  " + label;
    }

'''
if "function binding-glyph" not in ui:
    if function_marker not in ui:
        raise SystemExit("Slint function marker not found")
    ui = ui.replace(function_marker, function_insert + function_marker, 1)

replacements = {
    'action-label: "↖  " + root.numpad-seven-label;': 'action-label: root.binding-display(root.numpad-seven-label);',
    'action-label: "↑  " + root.numpad-eight-label;': 'action-label: root.binding-display(root.numpad-eight-label);',
    'action-label: "↗  " + root.numpad-nine-label;': 'action-label: root.binding-display(root.numpad-nine-label);',
    'action-label: "←  " + root.numpad-four-label;': 'action-label: root.binding-display(root.numpad-four-label);',
    'action-label: "→  " + root.numpad-six-label;': 'action-label: root.binding-display(root.numpad-six-label);',
    'action-label: "↙  " + root.numpad-one-label;': 'action-label: root.binding-display(root.numpad-one-label);',
    'action-label: "↓  " + root.numpad-two-label;': 'action-label: root.binding-display(root.numpad-two-label);',
    'action-label: "↘  " + root.numpad-three-label;': 'action-label: root.binding-display(root.numpad-three-label);',
    'action-label: root.numpad-five-label;': 'action-label: root.binding-display(root.numpad-five-label);',
    'action-label: root.numpad-zero-label;': 'action-label: root.binding-display(root.numpad-zero-label);',
    'action-label: root.numpad-add-label;': 'action-label: root.binding-display(root.numpad-add-label);',
    'action-label: root.numpad-decimal-label;': 'action-label: root.binding-display(root.numpad-decimal-label);',
}
for old, new in replacements.items():
    ui = ui.replace(old, new)

old_confirmation = '''                    Button {
                        text: "Reset all";
                        clicked => {
                            root.reset-defaults();
                            root.mark-saved();
                            reset-dialog.close();
                        }
                    }
'''
new_confirmation = '''                    UtilityButton {
                        text: "Reset all";
                        destructive: true;
                        min-width: 76px;
                        clicked => {
                            root.reset-defaults();
                            root.mark-saved();
                            reset-dialog.close();
                        }
                    }
'''
if old_confirmation not in ui:
    raise SystemExit("reset confirmation marker not found")
ui = ui.replace(old_confirmation, new_confirmation, 1)
UI.write_text(ui, encoding="utf-8")
