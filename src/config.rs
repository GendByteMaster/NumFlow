use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
};

use atomicwrites::{AllowOverwrite, AtomicFile};
use numflow_core::{Bindings, Direction, InputAction, MotionConfig, MouseButton, NumpadKey};
use serde::{Deserialize, Serialize};

pub const CONFIG_SCHEMA_VERSION: u32 = 1;
const CONFIG_FILE_NAME: &str = "config.toml";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigLoadStatus {
    Loaded,
    Missing,
    Recovered { reason: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConfigLoad {
    pub config: AppConfig,
    pub status: ConfigLoadStatus,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("unable to determine a per-user configuration directory")]
    MissingConfigDirectory,
    #[error("failed to read configuration: {0}")]
    Read(#[source] std::io::Error),
    #[error("failed to serialize configuration: {0}")]
    Serialize(#[source] toml::ser::Error),
    #[error("failed to write configuration atomically: {0}")]
    Write(String),
    #[error("configuration schema {found} is unsupported; expected {expected}")]
    UnsupportedSchema { found: u32, expected: u32 },
    #[error("active profile `{0}` does not exist")]
    MissingActiveProfile(String),
    #[error("profile `{profile}` contains duplicate binding for {key:?}")]
    DuplicateBinding {
        profile: String,
        key: NumpadKeyConfig,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppConfig {
    pub schema_version: u32,
    pub active_profile: String,
    pub hud_enabled: bool,
    pub start_minimized: bool,
    pub start_with_windows: bool,
    pub profiles: BTreeMap<String, ProfileConfig>,
}

impl Default for AppConfig {
    fn default() -> Self {
        let bindings = BindingConfig::defaults();
        let mut profiles = BTreeMap::new();

        profiles.insert(
            "Normal".to_owned(),
            ProfileConfig {
                speed: 180.0,
                max_speed: 1_400.0,
                acceleration: 900.0,
                precision_multiplier: 0.25,
                boost_multiplier: 1.8,
                precision_enabled: false,
                selected_button: MouseButtonConfig::Left,
                bindings: bindings.clone(),
            },
        );

        profiles.insert(
            "Precision".to_owned(),
            ProfileConfig {
                speed: 130.0,
                max_speed: 700.0,
                acceleration: 500.0,
                precision_multiplier: 0.18,
                boost_multiplier: 1.5,
                precision_enabled: true,
                selected_button: MouseButtonConfig::Left,
                bindings: bindings.clone(),
            },
        );

        profiles.insert(
            "Fast".to_owned(),
            ProfileConfig {
                speed: 300.0,
                max_speed: 2_200.0,
                acceleration: 1_600.0,
                precision_multiplier: 0.30,
                boost_multiplier: 2.0,
                precision_enabled: false,
                selected_button: MouseButtonConfig::Left,
                bindings,
            },
        );

        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            active_profile: "Normal".to_owned(),
            hud_enabled: true,
            start_minimized: false,
            start_with_windows: false,
            profiles,
        }
    }
}

impl AppConfig {
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.schema_version != CONFIG_SCHEMA_VERSION {
            return Err(ConfigError::UnsupportedSchema {
                found: self.schema_version,
                expected: CONFIG_SCHEMA_VERSION,
            });
        }

        if !self.profiles.contains_key(&self.active_profile) {
            return Err(ConfigError::MissingActiveProfile(
                self.active_profile.clone(),
            ));
        }

        for (profile_name, profile) in &self.profiles {
            profile.validate(profile_name)?;
        }

        Ok(())
    }

    #[must_use]
    pub fn active_profile(&self) -> &ProfileConfig {
        self.profiles
            .get(&self.active_profile)
            .expect("validated/default config always has an active profile")
    }

    pub fn set_active_profile(&mut self, profile: &str) -> Result<(), ConfigError> {
        if !self.profiles.contains_key(profile) {
            return Err(ConfigError::MissingActiveProfile(profile.to_owned()));
        }
        profile.clone_into(&mut self.active_profile);
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProfileConfig {
    pub speed: f64,
    pub max_speed: f64,
    pub acceleration: f64,
    pub precision_multiplier: f64,
    pub boost_multiplier: f64,
    pub precision_enabled: bool,
    pub selected_button: MouseButtonConfig,
    pub bindings: Vec<BindingConfig>,
}

impl ProfileConfig {
    fn validate(&self, profile_name: &str) -> Result<(), ConfigError> {
        let mut seen = BTreeSet::new();
        for binding in &self.bindings {
            if !seen.insert(binding.key) {
                return Err(ConfigError::DuplicateBinding {
                    profile: profile_name.to_owned(),
                    key: binding.key,
                });
            }
        }
        Ok(())
    }

    #[must_use]
    pub fn motion_config(&self) -> MotionConfig {
        MotionConfig {
            base_speed: self.speed,
            max_speed: self.max_speed,
            acceleration: self.acceleration,
            precision_multiplier: self.precision_multiplier,
            boost_multiplier: self.boost_multiplier,
        }
        .sanitized()
    }

    #[must_use]
    pub fn bindings(&self) -> Bindings {
        let mut bindings = Bindings::empty();
        for binding in &self.bindings {
            bindings.bind(binding.key.into(), binding.action.into());
        }
        bindings
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MouseButtonConfig {
    Left,
    Right,
    Middle,
}

impl From<MouseButtonConfig> for MouseButton {
    fn from(value: MouseButtonConfig) -> Self {
        match value {
            MouseButtonConfig::Left => Self::Left,
            MouseButtonConfig::Right => Self::Right,
            MouseButtonConfig::Middle => Self::Middle,
        }
    }
}

impl From<MouseButton> for MouseButtonConfig {
    fn from(value: MouseButton) -> Self {
        match value {
            MouseButton::Left => Self::Left,
            MouseButton::Right => Self::Right,
            MouseButton::Middle => Self::Middle,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct BindingConfig {
    pub key: NumpadKeyConfig,
    pub action: InputActionConfig,
}

impl BindingConfig {
    #[must_use]
    pub fn defaults() -> Vec<Self> {
        Bindings::default()
            .iter()
            .map(|(key, action)| Self {
                key: key.into(),
                action: action.into(),
            })
            .collect()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum NumpadKeyConfig {
    Num0,
    Num1,
    Num2,
    Num3,
    Num4,
    Num5,
    Num6,
    Num7,
    Num8,
    Num9,
    Add,
    Decimal,
    Divide,
    Multiply,
    Subtract,
}

impl From<NumpadKeyConfig> for NumpadKey {
    fn from(value: NumpadKeyConfig) -> Self {
        match value {
            NumpadKeyConfig::Num0 => Self::Num0,
            NumpadKeyConfig::Num1 => Self::Num1,
            NumpadKeyConfig::Num2 => Self::Num2,
            NumpadKeyConfig::Num3 => Self::Num3,
            NumpadKeyConfig::Num4 => Self::Num4,
            NumpadKeyConfig::Num5 => Self::Num5,
            NumpadKeyConfig::Num6 => Self::Num6,
            NumpadKeyConfig::Num7 => Self::Num7,
            NumpadKeyConfig::Num8 => Self::Num8,
            NumpadKeyConfig::Num9 => Self::Num9,
            NumpadKeyConfig::Add => Self::Add,
            NumpadKeyConfig::Decimal => Self::Decimal,
            NumpadKeyConfig::Divide => Self::Divide,
            NumpadKeyConfig::Multiply => Self::Multiply,
            NumpadKeyConfig::Subtract => Self::Subtract,
        }
    }
}

impl From<NumpadKey> for NumpadKeyConfig {
    fn from(value: NumpadKey) -> Self {
        match value {
            NumpadKey::Num0 => Self::Num0,
            NumpadKey::Num1 => Self::Num1,
            NumpadKey::Num2 => Self::Num2,
            NumpadKey::Num3 => Self::Num3,
            NumpadKey::Num4 => Self::Num4,
            NumpadKey::Num5 => Self::Num5,
            NumpadKey::Num6 => Self::Num6,
            NumpadKey::Num7 => Self::Num7,
            NumpadKey::Num8 => Self::Num8,
            NumpadKey::Num9 => Self::Num9,
            NumpadKey::Add => Self::Add,
            NumpadKey::Decimal => Self::Decimal,
            NumpadKey::Divide => Self::Divide,
            NumpadKey::Multiply => Self::Multiply,
            NumpadKey::Subtract => Self::Subtract,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DirectionConfig {
    Up,
    Down,
    Left,
    Right,
    UpLeft,
    UpRight,
    DownLeft,
    DownRight,
}

impl From<DirectionConfig> for Direction {
    fn from(value: DirectionConfig) -> Self {
        match value {
            DirectionConfig::Up => Self::Up,
            DirectionConfig::Down => Self::Down,
            DirectionConfig::Left => Self::Left,
            DirectionConfig::Right => Self::Right,
            DirectionConfig::UpLeft => Self::UpLeft,
            DirectionConfig::UpRight => Self::UpRight,
            DirectionConfig::DownLeft => Self::DownLeft,
            DirectionConfig::DownRight => Self::DownRight,
        }
    }
}

impl From<Direction> for DirectionConfig {
    fn from(value: Direction) -> Self {
        match value {
            Direction::Up => Self::Up,
            Direction::Down => Self::Down,
            Direction::Left => Self::Left,
            Direction::Right => Self::Right,
            Direction::UpLeft => Self::UpLeft,
            Direction::UpRight => Self::UpRight,
            Direction::DownLeft => Self::DownLeft,
            Direction::DownRight => Self::DownRight,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InputActionConfig {
    Move(DirectionConfig),
    Click,
    DoubleClick,
    Hold,
    Release,
    SelectButton(MouseButtonConfig),
    ToggleEnabled,
    SetEnabled(bool),
    SetPrecision(bool),
}

impl From<InputActionConfig> for InputAction {
    fn from(value: InputActionConfig) -> Self {
        match value {
            InputActionConfig::Move(direction) => Self::Move(direction.into()),
            InputActionConfig::Click => Self::Click,
            InputActionConfig::DoubleClick => Self::DoubleClick,
            InputActionConfig::Hold => Self::Hold,
            InputActionConfig::Release => Self::Release,
            InputActionConfig::SelectButton(button) => Self::SelectButton(button.into()),
            InputActionConfig::ToggleEnabled => Self::ToggleEnabled,
            InputActionConfig::SetEnabled(enabled) => Self::SetEnabled(enabled),
            InputActionConfig::SetPrecision(enabled) => Self::SetPrecision(enabled),
        }
    }
}

impl From<InputAction> for InputActionConfig {
    fn from(value: InputAction) -> Self {
        match value {
            InputAction::Move(direction) => Self::Move(direction.into()),
            InputAction::Click => Self::Click,
            InputAction::DoubleClick => Self::DoubleClick,
            InputAction::Hold => Self::Hold,
            InputAction::Release => Self::Release,
            InputAction::SelectButton(button) => Self::SelectButton(button.into()),
            InputAction::ToggleEnabled => Self::ToggleEnabled,
            InputAction::SetEnabled(enabled) => Self::SetEnabled(enabled),
            InputAction::SetPrecision(enabled) => Self::SetPrecision(enabled),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConfigStore {
    path: PathBuf,
}

impl ConfigStore {
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn for_current_user() -> Result<Self, ConfigError> {
        Ok(Self::new(default_config_path()?))
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load_or_default(&self) -> Result<ConfigLoad, ConfigError> {
        let contents = match fs::read_to_string(&self.path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ConfigLoad {
                    config: AppConfig::default(),
                    status: ConfigLoadStatus::Missing,
                });
            }
            Err(error) => return Err(ConfigError::Read(error)),
        };

        match toml::from_str::<AppConfig>(&contents) {
            Ok(config) => match config.validate() {
                Ok(()) => Ok(ConfigLoad {
                    config,
                    status: ConfigLoadStatus::Loaded,
                }),
                Err(error) => Ok(ConfigLoad {
                    config: AppConfig::default(),
                    status: ConfigLoadStatus::Recovered {
                        reason: error.to_string(),
                    },
                }),
            },
            Err(error) => Ok(ConfigLoad {
                config: AppConfig::default(),
                status: ConfigLoadStatus::Recovered {
                    reason: error.to_string(),
                },
            }),
        }
    }

    pub fn save(&self, config: &AppConfig) -> Result<(), ConfigError> {
        config.validate()?;

        let parent = self
            .path
            .parent()
            .ok_or(ConfigError::MissingConfigDirectory)?;
        fs::create_dir_all(parent).map_err(ConfigError::Read)?;

        let encoded = toml::to_string_pretty(config).map_err(ConfigError::Serialize)?;
        AtomicFile::new(&self.path, AllowOverwrite)
            .write(|file| {
                use std::io::Write;
                file.write_all(encoded.as_bytes())?;
                file.sync_all()
            })
            .map_err(|error| ConfigError::Write(error.to_string()))
    }
}

#[cfg(windows)]
fn default_config_path() -> Result<PathBuf, ConfigError> {
    let app_data = env::var_os("APPDATA").ok_or(ConfigError::MissingConfigDirectory)?;
    Ok(PathBuf::from(app_data)
        .join("NumFlow")
        .join(CONFIG_FILE_NAME))
}

#[cfg(not(windows))]
fn default_config_path() -> Result<PathBuf, ConfigError> {
    if let Some(config_home) = env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(config_home)
            .join("numflow")
            .join(CONFIG_FILE_NAME));
    }

    let home = env::var_os("HOME").ok_or(ConfigError::MissingConfigDirectory)?;
    Ok(PathBuf::from(home)
        .join(".config")
        .join("numflow")
        .join(CONFIG_FILE_NAME))
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use numflow_core::{Direction, InputAction, NumpadKey};

    use super::{
        AppConfig, BindingConfig, ConfigLoadStatus, ConfigStore, InputActionConfig, NumpadKeyConfig,
    };

    fn test_path(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("test clock should be after unix epoch")
            .as_nanos();
        std::env::temp_dir()
            .join(format!("numflow-{name}-{}-{nonce}", std::process::id()))
            .join("config.toml")
    }

    #[test]
    fn default_config_contains_required_profiles() {
        let config = AppConfig::default();

        assert_eq!(config.schema_version, 1);
        assert_eq!(config.active_profile, "Normal");
        assert!(config.profiles.contains_key("Normal"));
        assert!(config.profiles.contains_key("Precision"));
        assert!(config.profiles.contains_key("Fast"));
        assert!(config.validate().is_ok());
    }

    #[test]
    fn config_round_trip_preserves_profiles_and_bindings() {
        let path = test_path("round-trip");
        let store = ConfigStore::new(path.clone());
        let config = AppConfig {
            active_profile: "Precision".to_owned(),
            hud_enabled: false,
            ..AppConfig::default()
        };

        store.save(&config).expect("config should save");
        let loaded = store.load_or_default().expect("config should load");

        assert_eq!(loaded.status, ConfigLoadStatus::Loaded);
        assert_eq!(loaded.config, config);

        let _ = fs::remove_dir_all(path.parent().expect("test path has parent"));
    }

    #[test]
    fn corrupted_config_recovers_to_safe_defaults() {
        let path = test_path("corrupt");
        fs::create_dir_all(path.parent().expect("test path has parent"))
            .expect("test directory should exist");
        fs::write(&path, "this is not = [valid toml").expect("corrupted config should be written");

        let loaded = ConfigStore::new(path.clone())
            .load_or_default()
            .expect("corruption should recover");

        assert!(matches!(loaded.status, ConfigLoadStatus::Recovered { .. }));
        assert_eq!(loaded.config, AppConfig::default());

        let _ = fs::remove_dir_all(path.parent().expect("test path has parent"));
    }

    #[test]
    fn duplicate_binding_is_rejected() {
        let mut config = AppConfig::default();
        let profile = config
            .profiles
            .get_mut("Normal")
            .expect("default profile exists");
        profile.bindings.push(BindingConfig {
            key: NumpadKeyConfig::Num8,
            action: InputActionConfig::Click,
        });

        assert!(config.validate().is_err());
    }

    #[test]
    fn profile_bindings_convert_to_core_bindings() {
        let config = AppConfig::default();
        let bindings = config.active_profile().bindings();

        assert_eq!(
            bindings.action_for(NumpadKey::Num8),
            Some(InputAction::Move(Direction::Up))
        );
        assert_eq!(
            bindings.action_for(NumpadKey::Num5),
            Some(InputAction::Click)
        );
    }
}
