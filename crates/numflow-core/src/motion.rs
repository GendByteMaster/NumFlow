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
