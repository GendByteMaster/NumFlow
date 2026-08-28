use std::time::Duration;

use numflow_core::{
    Bindings, ControllerState, CoreEffect, Direction, InputAction, MotionConfig, MotionEngine,
    MotionModifiers, MouseButton, PointerEffect, StateChange,
};

#[test]
fn directions_are_unique_and_match_screen_coordinates() {
    let expected = [
        (Direction::Up, (0, -1)),
        (Direction::Down, (0, 1)),
        (Direction::Left, (-1, 0)),
        (Direction::Right, (1, 0)),
        (Direction::UpLeft, (-1, -1)),
        (Direction::UpRight, (1, -1)),
        (Direction::DownLeft, (-1, 1)),
        (Direction::DownRight, (1, 1)),
    ];
    let vectors = expected
        .iter()
        .map(|(_, vector)| *vector)
        .collect::<std::collections::HashSet<_>>();

    assert_eq!(vectors.len(), Direction::ALL.len());
    for (direction, vector) in expected {
        assert_eq!(direction.unit_vector(), vector);
    }
}

#[test]
fn default_bindings_cover_every_numpad_control() {
    let bindings = Bindings::default();
    let expected = [
        (8, InputAction::Move(Direction::Up)),
        (2, InputAction::Move(Direction::Down)),
        (4, InputAction::Move(Direction::Left)),
        (6, InputAction::Move(Direction::Right)),
        (7, InputAction::Move(Direction::UpLeft)),
        (9, InputAction::Move(Direction::UpRight)),
        (1, InputAction::Move(Direction::DownLeft)),
        (3, InputAction::Move(Direction::DownRight)),
        (5, InputAction::Click),
    ];

    for (digit, action) in expected {
        let key = match digit {
            1 => numflow_core::NumpadKey::Num1,
            2 => numflow_core::NumpadKey::Num2,
            3 => numflow_core::NumpadKey::Num3,
            4 => numflow_core::NumpadKey::Num4,
            5 => numflow_core::NumpadKey::Num5,
            6 => numflow_core::NumpadKey::Num6,
            7 => numflow_core::NumpadKey::Num7,
            8 => numflow_core::NumpadKey::Num8,
            9 => numflow_core::NumpadKey::Num9,
            _ => unreachable!("test fixture only contains NumPad digits"),
        };
        assert_eq!(bindings.action_for(key), Some(action));
    }

    assert_eq!(bindings.iter().count(), 15);
}

#[test]
fn bindings_can_be_reassigned_and_removed() {
    let mut bindings = Bindings::default();

    assert_eq!(
        bindings.bind(numflow_core::NumpadKey::Num5, InputAction::DoubleClick),
        Some(InputAction::Click)
    );
    assert_eq!(
        bindings.action_for(numflow_core::NumpadKey::Num5),
        Some(InputAction::DoubleClick)
    );
    assert_eq!(
        bindings.unbind(numflow_core::NumpadKey::Num5),
        Some(InputAction::DoubleClick)
    );
    assert_eq!(bindings.action_for(numflow_core::NumpadKey::Num5), None);
}

#[test]
fn controller_emits_click_hold_release_and_fail_safe_effects() {
    let mut state = ControllerState::default();
    assert!(!state.is_enabled());
    assert!(!state.is_dragging());

    assert_eq!(
        state.apply(InputAction::SetEnabled(true)),
        vec![CoreEffect::State(StateChange::Enabled(true))]
    );
    state.apply(InputAction::SelectButton(MouseButton::Right));
    assert_eq!(
        state.apply(InputAction::Hold),
        vec![CoreEffect::Pointer(PointerEffect::ButtonDown(
            MouseButton::Right
        ))]
    );
    assert!(state.apply(InputAction::Hold).is_empty());
    assert_eq!(state.held_button(), Some(MouseButton::Right));

    assert_eq!(
        state.apply(InputAction::SetEnabled(false)),
        vec![
            CoreEffect::Pointer(PointerEffect::ButtonUp(MouseButton::Right)),
            CoreEffect::State(StateChange::Enabled(false)),
        ]
    );
    assert!(!state.is_dragging());
    assert_eq!(state.held_button(), None);
    assert!(state.shutdown().is_empty());
}

#[test]
fn controller_suppresses_pointer_clicks_during_drag_but_allows_movement() {
    let mut state = ControllerState::default();
    state.apply(InputAction::SetEnabled(true));
    state.apply(InputAction::Hold);

    assert!(state.apply(InputAction::Click).is_empty());
    assert!(state.apply(InputAction::DoubleClick).is_empty());
    assert_eq!(
        state.apply(InputAction::Move(Direction::Right)),
        vec![CoreEffect::Pointer(PointerEffect::Move(Direction::Right))]
    );
}

#[test]
fn controller_keeps_held_button_stable_when_selection_changes() {
    let mut state = ControllerState::default();
    state.apply(InputAction::SetEnabled(true));
    state.apply(InputAction::SelectButton(MouseButton::Right));
    state.apply(InputAction::Hold);

    assert_eq!(
        state.apply(InputAction::SelectButton(MouseButton::Middle)),
        vec![CoreEffect::State(StateChange::SelectedButton(
            MouseButton::Middle
        ))]
    );
    assert_eq!(state.selected_button(), MouseButton::Middle);
    assert_eq!(state.held_button(), Some(MouseButton::Right));
    assert_eq!(
        state.apply(InputAction::Release),
        vec![CoreEffect::Pointer(PointerEffect::ButtonUp(
            MouseButton::Right
        ))]
    );
}

#[test]
fn controller_toggle_and_precision_are_independent() {
    let mut state = ControllerState::default();
    assert_eq!(
        state.apply(InputAction::ToggleEnabled),
        vec![CoreEffect::State(StateChange::Enabled(true))]
    );
    assert_eq!(
        state.apply(InputAction::ToggleEnabled),
        vec![CoreEffect::State(StateChange::Enabled(false))]
    );
    assert_eq!(
        state.apply(InputAction::SetPrecision(true)),
        vec![CoreEffect::State(StateChange::Precision(true))]
    );
    assert!(state.is_precision_enabled());
    assert!(!state.is_enabled());
}

fn constant_speed_config(speed: f64) -> MotionConfig {
    MotionConfig {
        base_speed: speed,
        max_speed: speed,
        acceleration: 0.0,
        precision_multiplier: 0.25,
        boost_multiplier: 2.0,
    }
}

#[test]
fn motion_produces_cardinal_and_normalized_diagonal_steps() {
    let mut cardinal = MotionEngine::new(constant_speed_config(100.0));
    cardinal.press(Direction::Right);
    let cardinal_step = cardinal
        .tick(Duration::from_secs(1), MotionModifiers::default())
        .expect("active movement should produce a step");
    assert_eq!((cardinal_step.dx, cardinal_step.dy), (100, 0));

    let mut diagonal = MotionEngine::new(constant_speed_config(1_000.0));
    diagonal.press(Direction::UpRight);
    let diagonal_step = diagonal
        .tick(Duration::from_secs(1), MotionModifiers::default())
        .expect("active movement should produce a step");
    assert_eq!((diagonal_step.dx, diagonal_step.dy), (707, -707));
    assert!(
        (f64::hypot(f64::from(diagonal_step.dx), f64::from(diagonal_step.dy)) - 1_000.0).abs()
            < 1.0
    );
}

#[test]
fn motion_accelerates_without_exceeding_maximum() {
    let mut engine = MotionEngine::new(MotionConfig {
        base_speed: 100.0,
        max_speed: 300.0,
        acceleration: 100.0,
        ..MotionConfig::default()
    });
    engine.press(Direction::Right);

    let first = engine
        .tick(Duration::from_millis(500), MotionModifiers::default())
        .expect("first step should exist");
    let second = engine
        .tick(Duration::from_millis(500), MotionModifiers::default())
        .expect("second step should exist");
    let final_step = engine
        .tick(Duration::from_secs(10), MotionModifiers::default())
        .expect("final step should exist");

    assert!(first.speed < second.speed);
    assert!(second.speed <= final_step.speed);
    assert!((final_step.speed - 300.0).abs() < f64::EPSILON);
}

#[test]
fn motion_modifiers_scale_distance_and_speed() {
    let mut normal = MotionEngine::new(constant_speed_config(400.0));
    let mut precise = MotionEngine::new(constant_speed_config(400.0));
    let mut boosted = MotionEngine::new(constant_speed_config(100.0));
    normal.press(Direction::Right);
    precise.press(Direction::Right);
    boosted.press(Direction::Down);

    let normal_step = normal
        .tick(Duration::from_secs(1), MotionModifiers::default())
        .expect("normal movement should produce a step");
    let precise_step = precise
        .tick(
            Duration::from_secs(1),
            MotionModifiers {
                precision: true,
                boost: false,
            },
        )
        .expect("precision movement should produce a step");
    let boosted_step = boosted
        .tick(
            Duration::from_secs(1),
            MotionModifiers {
                precision: false,
                boost: true,
            },
        )
        .expect("boosted movement should produce a step");

    assert_eq!(normal_step.dx, 400);
    assert_eq!(precise_step.dx, 100);
    assert_eq!(boosted_step.dy, 200);
}

#[test]
fn motion_stops_and_resets_after_last_direction_release() {
    let mut engine = MotionEngine::new(constant_speed_config(100.0));
    engine.press(Direction::Left);
    assert!(
        engine
            .tick(Duration::from_millis(16), MotionModifiers::default())
            .is_some()
    );

    engine.release(Direction::Left);

    assert!(!engine.is_moving());
    assert_eq!(engine.elapsed(), Duration::ZERO);
    assert!(
        engine
            .tick(Duration::from_secs(1), MotionModifiers::default())
            .is_none()
    );
}

#[test]
fn motion_is_frame_rate_independent_and_sanitizes_invalid_settings() {
    let config = MotionConfig {
        base_speed: 100.0,
        max_speed: 500.0,
        acceleration: 200.0,
        ..MotionConfig::default()
    };
    let mut one_tick = MotionEngine::new(config);
    let mut ten_ticks = MotionEngine::new(config);
    one_tick.press(Direction::Right);
    ten_ticks.press(Direction::Right);

    let single = one_tick
        .tick(Duration::from_secs(1), MotionModifiers::default())
        .expect("single tick should produce movement");
    let mut accumulated = 0;
    for _ in 0..10 {
        accumulated += ten_ticks
            .tick(Duration::from_millis(100), MotionModifiers::default())
            .expect("split tick should produce movement")
            .dx;
    }
    assert_eq!(single.dx, accumulated);
    assert_eq!(one_tick.elapsed(), ten_ticks.elapsed());

    let sanitized = MotionEngine::new(MotionConfig {
        base_speed: f64::NAN,
        max_speed: -100.0,
        acceleration: f64::INFINITY,
        precision_multiplier: 0.0,
        boost_multiplier: 100.0,
    })
    .config();
    assert!(sanitized.base_speed.is_finite());
    assert!(sanitized.max_speed >= sanitized.base_speed);
    assert!(sanitized.acceleration.is_finite());
    assert!(sanitized.precision_multiplier >= 0.05);
    assert!(sanitized.boost_multiplier <= 10.0);
}
