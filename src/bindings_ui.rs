use crate::config::{
    BindingConfig, DirectionConfig, InputActionConfig, MouseButtonConfig, NumpadKeyConfig,
    ProfileConfig,
};

pub(crate) const BINDING_KEYS: [NumpadKeyConfig; 15] = [
    NumpadKeyConfig::Num8,
    NumpadKeyConfig::Num2,
    NumpadKeyConfig::Num4,
    NumpadKeyConfig::Num6,
    NumpadKeyConfig::Num7,
    NumpadKeyConfig::Num9,
    NumpadKeyConfig::Num1,
    NumpadKeyConfig::Num3,
    NumpadKeyConfig::Num5,
    NumpadKeyConfig::Add,
    NumpadKeyConfig::Num0,
    NumpadKeyConfig::Decimal,
    NumpadKeyConfig::Divide,
    NumpadKeyConfig::Multiply,
    NumpadKeyConfig::Subtract,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BindingChoice {
    Unbound,
    Action(InputActionConfig),
}

impl BindingChoice {
    #[must_use]
    pub(crate) const fn action(self) -> Option<InputActionConfig> {
        match self {
            Self::Unbound => None,
            Self::Action(action) => Some(action),
        }
    }
}

pub(crate) const BINDING_CHOICES: [BindingChoice; 18] = [
    BindingChoice::Unbound,
    BindingChoice::Action(InputActionConfig::Move(DirectionConfig::Up)),
    BindingChoice::Action(InputActionConfig::Move(DirectionConfig::Down)),
    BindingChoice::Action(InputActionConfig::Move(DirectionConfig::Left)),
    BindingChoice::Action(InputActionConfig::Move(DirectionConfig::Right)),
    BindingChoice::Action(InputActionConfig::Move(DirectionConfig::UpLeft)),
    BindingChoice::Action(InputActionConfig::Move(DirectionConfig::UpRight)),
    BindingChoice::Action(InputActionConfig::Move(DirectionConfig::DownLeft)),
    BindingChoice::Action(InputActionConfig::Move(DirectionConfig::DownRight)),
    BindingChoice::Action(InputActionConfig::Click),
    BindingChoice::Action(InputActionConfig::DoubleClick),
    BindingChoice::Action(InputActionConfig::Hold),
    BindingChoice::Action(InputActionConfig::Release),
    BindingChoice::Action(InputActionConfig::SelectButton(MouseButtonConfig::Left)),
    BindingChoice::Action(InputActionConfig::SelectButton(MouseButtonConfig::Right)),
    BindingChoice::Action(InputActionConfig::SelectButton(MouseButtonConfig::Middle)),
    BindingChoice::Action(InputActionConfig::SetPrecision(true)),
    BindingChoice::Action(InputActionConfig::SetPrecision(false)),
];

#[must_use]
pub(crate) fn key_from_index(index: i32) -> Option<NumpadKeyConfig> {
    usize::try_from(index)
        .ok()
        .and_then(|index| BINDING_KEYS.get(index).copied())
}

#[must_use]
pub(crate) fn key_index(key: NumpadKeyConfig) -> i32 {
    BINDING_KEYS
        .iter()
        .position(|candidate| *candidate == key)
        .and_then(|index| i32::try_from(index).ok())
        .unwrap_or_default()
}

#[must_use]
pub(crate) fn choice_from_index(index: i32) -> Option<BindingChoice> {
    usize::try_from(index)
        .ok()
        .and_then(|index| BINDING_CHOICES.get(index).copied())
}

#[must_use]
pub(crate) fn choice_index(action: Option<InputActionConfig>) -> i32 {
    BINDING_CHOICES
        .iter()
        .position(|choice| choice.action() == action)
        .and_then(|index| i32::try_from(index).ok())
        .unwrap_or_default()
}

#[must_use]
pub(crate) fn profile_action(
    profile: &ProfileConfig,
    key: NumpadKeyConfig,
) -> Option<InputActionConfig> {
    profile
        .bindings
        .iter()
        .find(|binding| binding.key == key)
        .map(|binding| binding.action)
}

pub(crate) fn set_profile_binding(
    profile: &mut ProfileConfig,
    key: NumpadKeyConfig,
    action: Option<InputActionConfig>,
) -> bool {
    let current = profile_action(profile, key);
    if current == action {
        return false;
    }

    match (
        profile
            .bindings
            .iter()
            .position(|binding| binding.key == key),
        action,
    ) {
        (Some(index), Some(action)) => profile.bindings[index].action = action,
        (Some(index), None) => {
            profile.bindings.remove(index);
        }
        (None, Some(action)) => profile.bindings.push(BindingConfig { key, action }),
        (None, None) => {}
    }

    profile.bindings.sort_by_key(|binding| binding.key);
    true
}

pub(crate) fn reset_profile_bindings(profile: &mut ProfileConfig) -> bool {
    let defaults = BindingConfig::defaults();
    if profile.bindings == defaults {
        return false;
    }

    profile.bindings = defaults;
    true
}

#[must_use]
pub(crate) const fn action_label(action: Option<InputActionConfig>) -> &'static str {
    match action {
        None => "Unbound",
        Some(InputActionConfig::Move(DirectionConfig::Up)) => "Up",
        Some(InputActionConfig::Move(DirectionConfig::Down)) => "Down",
        Some(InputActionConfig::Move(DirectionConfig::Left)) => "Left",
        Some(InputActionConfig::Move(DirectionConfig::Right)) => "Right",
        Some(InputActionConfig::Move(DirectionConfig::UpLeft)) => "Up-left",
        Some(InputActionConfig::Move(DirectionConfig::UpRight)) => "Up-right",
        Some(InputActionConfig::Move(DirectionConfig::DownLeft)) => "Down-left",
        Some(InputActionConfig::Move(DirectionConfig::DownRight)) => "Down-right",
        Some(InputActionConfig::Click) => "Click",
        Some(InputActionConfig::DoubleClick) => "Double click",
        Some(InputActionConfig::Hold) => "Hold / drag",
        Some(InputActionConfig::Release) => "Release",
        Some(InputActionConfig::SelectButton(MouseButtonConfig::Left)) => "Select left",
        Some(InputActionConfig::SelectButton(MouseButtonConfig::Right)) => "Select right",
        Some(InputActionConfig::SelectButton(MouseButtonConfig::Middle)) => "Select middle",
        Some(InputActionConfig::ToggleEnabled) => "Toggle NumFlow",
        Some(InputActionConfig::SetEnabled(true)) => "Enable NumFlow",
        Some(InputActionConfig::SetEnabled(false)) => "Disable NumFlow",
        Some(InputActionConfig::SetPrecision(true)) => "Precision on",
        Some(InputActionConfig::SetPrecision(false)) => "Precision off",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BINDING_CHOICES, BINDING_KEYS, BindingChoice, choice_from_index, choice_index,
        key_from_index, key_index, profile_action, reset_profile_bindings, set_profile_binding,
    };
    use crate::config::{AppConfig, InputActionConfig, NumpadKeyConfig};

    #[test]
    fn binding_key_indices_round_trip() {
        for key in BINDING_KEYS {
            assert_eq!(key_from_index(key_index(key)), Some(key));
        }
    }

    #[test]
    fn binding_choice_indices_round_trip() {
        for choice in BINDING_CHOICES {
            assert_eq!(
                choice_from_index(choice_index(choice.action())),
                Some(choice)
            );
        }
        assert_eq!(choice_from_index(-1), None);
        assert_eq!(choice_from_index(999), None);
    }

    #[test]
    fn profile_binding_can_be_reassigned_and_unbound_without_duplicates() {
        let mut config = AppConfig::default();
        let profile = config
            .profiles
            .get_mut("Normal")
            .expect("Normal profile should exist");

        assert!(set_profile_binding(
            profile,
            NumpadKeyConfig::Num5,
            Some(InputActionConfig::DoubleClick),
        ));
        assert_eq!(
            profile_action(profile, NumpadKeyConfig::Num5),
            Some(InputActionConfig::DoubleClick)
        );
        assert_eq!(
            profile
                .bindings
                .iter()
                .filter(|binding| binding.key == NumpadKeyConfig::Num5)
                .count(),
            1
        );

        assert!(set_profile_binding(profile, NumpadKeyConfig::Num5, None));
        assert_eq!(profile_action(profile, NumpadKeyConfig::Num5), None);
        assert!(!set_profile_binding(profile, NumpadKeyConfig::Num5, None));
    }

    #[test]
    fn reset_restores_default_bindings() {
        let mut config = AppConfig::default();
        let profile = config
            .profiles
            .get_mut("Normal")
            .expect("Normal profile should exist");

        set_profile_binding(profile, NumpadKeyConfig::Num8, None);
        assert!(reset_profile_bindings(profile));
        assert_eq!(
            profile_action(profile, NumpadKeyConfig::Num8),
            BindingChoice::Action(InputActionConfig::Move(crate::config::DirectionConfig::Up))
                .action()
        );
        assert!(!reset_profile_bindings(profile));
    }
}
