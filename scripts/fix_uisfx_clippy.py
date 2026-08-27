from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    file = Path(path)
    text = file.read_text(encoding="utf-8")
    count = text.count(old)
    if count != 1:
        raise RuntimeError(f"expected one match in {path}, found {count}: {old[:120]!r}")
    file.write_text(text.replace(old, new, 1), encoding="utf-8")


# Keep the serialized key as `sounds_enabled = true`, while avoiding another raw bool in AppConfig.
replace_once(
    "src/config.rs",
    '''const fn default_sounds_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppConfig {
''',
    '''#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct InterfaceSoundsEnabled(bool);

impl InterfaceSoundsEnabled {
    pub const ENABLED: Self = Self(true);

    #[must_use]
    pub const fn get(self) -> bool {
        self.0
    }
}

impl Default for InterfaceSoundsEnabled {
    fn default() -> Self {
        Self::ENABLED
    }
}

impl From<bool> for InterfaceSoundsEnabled {
    fn from(enabled: bool) -> Self {
        Self(enabled)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppConfig {
''',
)
replace_once(
    "src/config.rs",
    '''    pub hud_enabled: bool,
    #[serde(default = "default_sounds_enabled")]
    pub sounds_enabled: bool,
''',
    '''    pub hud_enabled: bool,
    #[serde(default)]
    pub sounds_enabled: InterfaceSoundsEnabled,
''',
)
replace_once(
    "src/config.rs",
    "            sounds_enabled: true,\n",
    "            sounds_enabled: InterfaceSoundsEnabled::ENABLED,\n",
)
replace_once(
    "src/config.rs",
    "        assert!(parsed.sounds_enabled);\n",
    "        assert!(parsed.sounds_enabled.get());\n",
)
replace_once(
    "src/app.rs",
    "        self.config.sounds_enabled = enabled;\n",
    "        self.config.sounds_enabled = enabled.into();\n",
)
replace_once(
    "src/app.rs",
    "        self.config.sounds_enabled\n",
    "        self.config.sounds_enabled.get()\n",
)

# Keep connect_preferences focused on preferences that already existed; sound wiring is separate.
app = Path("src/app.rs")
text = app.read_text(encoding="utf-8")
start = text.index("fn connect_preferences(\n")
first_hud_marker = "    {\n        let settings = Rc::clone(settings);\n        let hud = Rc::clone(hud);\n        let store = Rc::clone(store);\n        window.on_hud_toggled"
hud_pos = text.index(first_hud_marker, start)
header_end = text.index(") {\n", start) + len(") {\n")
sound_blocks = text[header_end:hud_pos]
if "on_sounds_toggled" not in sound_blocks or "on_ui_sound_requested" not in sound_blocks:
    raise RuntimeError("sound preference blocks were not found at the start of connect_preferences")
text = text[:header_end] + text[hud_pos:]
helper = '''fn connect_sound_preferences(
    window: &AppWindow,
    settings: &SharedUiSettings,
    store: &SharedConfigStore,
    runtime: &SharedRuntime,
) {
''' + sound_blocks + '''}

'''
insert_at = text.index("fn connect_preferences(\n")
text = text[:insert_at] + helper + text[insert_at:]
app.write_text(text, encoding="utf-8")
replace_once(
    "src/app.rs",
    '''    connect_pointer_controls(window, tray, settings, hud, store, runtime);
    connect_binding_controls(window, settings, store, runtime);
    connect_preferences(window, tray, settings, hud, store, runtime);
''',
    '''    connect_pointer_controls(window, tray, settings, hud, store, runtime);
    connect_binding_controls(window, settings, store, runtime);
    connect_sound_preferences(window, settings, store, runtime);
    connect_preferences(window, tray, settings, hud, store, runtime);
''',
)

# Pull audio initialization out of worker_main to keep the runtime loop readable and under lint limits.
replace_once(
    "src/runtime.rs",
    '''        let audio_feedback = match AudioFeedbackService::start() {
            Ok(service) => {
                service.set_enabled(config.sounds_enabled);
                Some(service)
            }
            Err(error) => {
                tracing::warn!(%error, "NumFlow audio feedback is unavailable");
                None
            }
        };
''',
    "        let audio_feedback = start_audio_feedback(config.sounds_enabled);\n",
)
replace_once(
    "src/runtime.rs",
    '''    fn worker_main(
        config: RuntimeConfig,
''',
    '''    fn start_audio_feedback(enabled: bool) -> Option<AudioFeedbackService> {
        match AudioFeedbackService::start() {
            Ok(service) => {
                service.set_enabled(enabled);
                Some(service)
            }
            Err(error) => {
                tracing::warn!(%error, "NumFlow audio feedback is unavailable");
                None
            }
        }
    }

    fn worker_main(
        config: RuntimeConfig,
''',
)

# Keep the asset notice clean and explicit.
Path("assets/sfx/README.md").write_text(
    """# NumFlow UI sound assets

NumFlow uses selected semantic cues from the **UI SFX Glass** pack. The upstream generated audio is dedicated to the public domain under **CC0 1.0**.

Source: https://github.com/romainsimon/uisfx/tree/main/packages/uisfx/sounds/glass

The repository stores PCM WAV conversions (mono, 22.05 kHz, 16-bit) so the Windows native build can play them directly with `PlaySound` without a heavy audio dependency.

Included semantic cues: toggle-on/off, select, open/close, expand/collapse, drag-start, release, delete, and error. Pointer motion and ordinary clicks intentionally remain silent.
""",
    encoding="utf-8",
)
