use std::time::Duration;

use num_traits::ToPrimitive;

use crate::Direction;

const MIN_SPEED: f64 = 1.0;
const MAX_SPEED: f64 = 20_000.0;
const MAX_ACCELERATION: f64 = 50_000.0;
const MIN_MULTIPLIER: f64 = 0.05;
const MAX_MULTIPLIER: f64 = 10.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionConfig {
    pub base_speed: f64,
    pub max_speed: f64,
    pub acceleration: f64,
    pub precision_multiplier: f64,
    pub boost_multiplier: f64,
}

impl Default for MotionConfig {
    fn default() -> Self {
        Self {
            base_speed: 180.0,
            max_speed: 1_400.0,
            acceleration: 900.0,
            precision_multiplier: 0.25,
            boost_multiplier: 1.8,
        }
    }
}

impl MotionConfig {
    #[must_use]
    pub fn sanitized(self) -> Self {
        let defaults = Self::default();
        let base_speed = sanitize_f64(self.base_speed, defaults.base_speed, MIN_SPEED, MAX_SPEED);
        let max_speed =
            sanitize_f64(self.max_speed, defaults.max_speed, MIN_SPEED, MAX_SPEED).max(base_speed);

        Self {
            base_speed,
            max_speed,
            acceleration: sanitize_f64(
                self.acceleration,
                defaults.acceleration,
                0.0,
                MAX_ACCELERATION,
            ),
            precision_multiplier: sanitize_f64(
                self.precision_multiplier,
                defaults.precision_multiplier,
                MIN_MULTIPLIER,
                MAX_MULTIPLIER,
            ),
            boost_multiplier: sanitize_f64(
                self.boost_multiplier,
                defaults.boost_multiplier,
                MIN_MULTIPLIER,
                MAX_MULTIPLIER,
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MotionModifiers {
    pub precision: bool,
    pub boost: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionStep {
    pub dx: i32,
    pub dy: i32,
    pub speed: f64,
}

#[derive(Debug, Clone)]
pub struct MotionEngine {
    config: MotionConfig,
    active_directions: u8,
    movement_elapsed: Duration,
    residual_x: f64,
    residual_y: f64,
}

impl Default for MotionEngine {
    fn default() -> Self {
        Self::new(MotionConfig::default())
    }
}

impl MotionEngine {
    #[must_use]
    pub fn new(config: MotionConfig) -> Self {
        Self {
            config: config.sanitized(),
            active_directions: 0,
            movement_elapsed: Duration::ZERO,
            residual_x: 0.0,
            residual_y: 0.0,
        }
    }

    #[must_use]
    pub const fn config(&self) -> MotionConfig {
        self.config
    }

    pub fn set_config(&mut self, config: MotionConfig) {
        self.config = config.sanitized();
    }

    #[must_use]
    pub const fn is_moving(&self) -> bool {
        self.active_directions != 0
    }

    #[must_use]
    pub const fn elapsed(&self) -> Duration {
        self.movement_elapsed
    }

    pub fn press(&mut self, direction: Direction) {
        if self.active_directions == 0 {
            self.reset_motion_progress();
        }
        self.active_directions |= direction_bit(direction);
    }

    pub fn release(&mut self, direction: Direction) {
        self.active_directions &= !direction_bit(direction);
        if self.active_directions == 0 {
            self.reset_motion_progress();
        }
    }

    pub fn stop(&mut self) {
        self.active_directions = 0;
        self.reset_motion_progress();
    }

    #[must_use]
    pub fn current_speed(&self, modifiers: MotionModifiers) -> f64 {
        speed_at(
            self.movement_elapsed.as_secs_f64(),
            self.config,
            modifier_scale(modifiers, self.config),
        )
    }

    #[must_use]
    pub fn tick(&mut self, elapsed: Duration, modifiers: MotionModifiers) -> Option<MotionStep> {
        if elapsed.is_zero() || self.active_directions == 0 {
            return None;
        }

        let Some((unit_x, unit_y)) = normalized_vector(self.active_directions) else {
            self.reset_motion_progress();
            return None;
        };

        let start = self.movement_elapsed.as_secs_f64();
        self.movement_elapsed = self.movement_elapsed.saturating_add(elapsed);
        let end = self.movement_elapsed.as_secs_f64();
        let scale = modifier_scale(modifiers, self.config);
        let distance = integrated_distance(start, end, self.config) * scale;

        let total_x = self.residual_x + unit_x * distance;
        let total_y = self.residual_y + unit_y * distance;
        let dx = saturating_trunc_i32(total_x);
        let dy = saturating_trunc_i32(total_y);
        self.residual_x = total_x - f64::from(dx);
        self.residual_y = total_y - f64::from(dy);

        Some(MotionStep {
            dx,
            dy,
            speed: speed_at(end, self.config, scale),
        })
    }

    fn reset_motion_progress(&mut self) {
        self.movement_elapsed = Duration::ZERO;
        self.residual_x = 0.0;
        self.residual_y = 0.0;
    }
}

fn sanitize_f64(value: f64, fallback: f64, min: f64, max: f64) -> f64 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        fallback
    }
}

const fn direction_bit(direction: Direction) -> u8 {
    1 << match direction {
        Direction::Up => 0,
        Direction::Down => 1,
        Direction::Left => 2,
        Direction::Right => 3,
        Direction::UpLeft => 4,
        Direction::UpRight => 5,
        Direction::DownLeft => 6,
        Direction::DownRight => 7,
    }
}

fn normalized_vector(active_directions: u8) -> Option<(f64, f64)> {
    let mut x = 0.0;
    let mut y = 0.0;

    for direction in Direction::ALL {
        if active_directions & direction_bit(direction) == 0 {
            continue;
        }

        let (dx, dy) = direction.unit_vector();
        x += f64::from(dx);
        y += f64::from(dy);
    }

    let magnitude = f64::hypot(x, y);
    if magnitude <= f64::EPSILON {
        None
    } else {
        Some((x / magnitude, y / magnitude))
    }
}

fn modifier_scale(modifiers: MotionModifiers, config: MotionConfig) -> f64 {
    let precision = if modifiers.precision {
        config.precision_multiplier
    } else {
        1.0
    };
    let boost = if modifiers.boost {
        config.boost_multiplier
    } else {
        1.0
    };
    precision * boost
}

fn speed_at(elapsed_seconds: f64, config: MotionConfig, scale: f64) -> f64 {
    (config.base_speed + config.acceleration * elapsed_seconds).min(config.max_speed) * scale
}

fn integrated_distance(start: f64, end: f64, config: MotionConfig) -> f64 {
    if end <= start {
        return 0.0;
    }

    if config.acceleration <= f64::EPSILON || config.base_speed >= config.max_speed {
        return config.base_speed * (end - start);
    }

    let time_to_max = (config.max_speed - config.base_speed) / config.acceleration;
    let accelerating_end = end.min(time_to_max);
    let accelerating_start = start.min(time_to_max);

    let accelerating_distance = if accelerating_end > accelerating_start {
        config.base_speed * (accelerating_end - accelerating_start)
            + 0.5 * config.acceleration * (accelerating_end.powi(2) - accelerating_start.powi(2))
    } else {
        0.0
    };

    let capped_start = start.max(time_to_max);
    let capped_distance = if end > capped_start {
        config.max_speed * (end - capped_start)
    } else {
        0.0
    };

    accelerating_distance + capped_distance
}

fn saturating_trunc_i32(value: f64) -> i32 {
    if !value.is_finite() {
        return 0;
    }

    let clamped = value
        .clamp(f64::from(i32::MIN), f64::from(i32::MAX))
        .trunc();
    clamped.to_i32().unwrap_or(if clamped.is_sign_negative() {
        i32::MIN
    } else {
        i32::MAX
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{MotionConfig, MotionEngine, MotionModifiers};
    use crate::Direction;

    const FLOAT_EPSILON: f64 = 1.0e-9;

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() <= FLOAT_EPSILON,
            "expected {expected}, got {actual}"
        );
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
    fn cardinal_velocity_matches_configured_speed() {
        let mut engine = MotionEngine::new(constant_speed_config(100.0));
        engine.press(Direction::Right);

        let step = engine
            .tick(Duration::from_secs(1), MotionModifiers::default())
            .expect("active movement should produce a step");

        assert_eq!((step.dx, step.dy), (100, 0));
        assert_close(step.speed, 100.0);
    }

    #[test]
    fn diagonal_velocity_is_normalized() {
        let mut engine = MotionEngine::new(constant_speed_config(1_000.0));
        engine.press(Direction::UpRight);

        let step = engine
            .tick(Duration::from_secs(1), MotionModifiers::default())
            .expect("diagonal movement should produce a step");

        assert_eq!(step.dx, 707);
        assert_eq!(step.dy, -707);
        assert!((f64::hypot(f64::from(step.dx), f64::from(step.dy)) - 1_000.0).abs() < 1.0);
    }

    #[test]
    fn acceleration_is_monotonic_until_max_speed() {
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
        let third = engine
            .tick(Duration::from_secs(2), MotionModifiers::default())
            .expect("third step should exist");

        assert!(first.speed < second.speed);
        assert!(second.speed <= third.speed);
        assert_close(third.speed, 300.0);
    }

    #[test]
    fn speed_never_exceeds_maximum() {
        let mut engine = MotionEngine::new(MotionConfig {
            base_speed: 100.0,
            max_speed: 200.0,
            acceleration: 10_000.0,
            ..MotionConfig::default()
        });
        engine.press(Direction::Down);

        let step = engine
            .tick(Duration::from_secs(10), MotionModifiers::default())
            .expect("movement should produce a step");

        assert_close(step.speed, 200.0);
    }

    #[test]
    fn precision_multiplier_reduces_distance_and_speed() {
        let mut normal = MotionEngine::new(constant_speed_config(400.0));
        let mut precise = MotionEngine::new(constant_speed_config(400.0));
        normal.press(Direction::Right);
        precise.press(Direction::Right);

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

        assert_eq!(normal_step.dx, 400);
        assert_eq!(precise_step.dx, 100);
        assert_close(precise_step.speed, 100.0);
    }

    #[test]
    fn boost_multiplier_increases_distance() {
        let mut engine = MotionEngine::new(constant_speed_config(100.0));
        engine.press(Direction::Down);

        let step = engine
            .tick(
                Duration::from_secs(1),
                MotionModifiers {
                    precision: false,
                    boost: true,
                },
            )
            .expect("boosted movement should produce a step");

        assert_eq!(step.dy, 200);
        assert_close(step.speed, 200.0);
    }

    #[test]
    fn releasing_last_direction_stops_immediately() {
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
    fn elapsed_time_produces_frame_rate_independent_distance() {
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
    }

    #[test]
    fn opposing_directions_cancel_without_building_acceleration() {
        let mut engine = MotionEngine::new(MotionConfig::default());
        engine.press(Direction::Left);
        engine.press(Direction::Right);

        assert!(
            engine
                .tick(Duration::from_secs(1), MotionModifiers::default())
                .is_none()
        );
        assert_eq!(engine.elapsed(), Duration::ZERO);
    }

    #[test]
    fn invalid_and_extreme_settings_are_sanitized() {
        let engine = MotionEngine::new(MotionConfig {
            base_speed: f64::NAN,
            max_speed: -100.0,
            acceleration: f64::INFINITY,
            precision_multiplier: 0.0,
            boost_multiplier: 100.0,
        });
        let config = engine.config();

        assert!(config.base_speed.is_finite());
        assert!(config.max_speed >= config.base_speed);
        assert!(config.acceleration.is_finite());
        assert!(config.precision_multiplier >= 0.05);
        assert!(config.boost_multiplier <= 10.0);
    }
}
