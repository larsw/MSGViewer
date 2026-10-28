//! The zoom levels the Body steps through, like a web browser's.

/// From smallest to largest; 1.0 is the Body's own size.
const LEVELS: [f64; 13] = [
    0.5, 0.67, 0.75, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0,
];

pub const DEFAULT: f64 = 1.0;

/// Tolerance for a level that went through floating-point arithmetic.
const EPSILON: f64 = 0.001;

/// The next larger level, or the largest one.
pub fn larger(level: f64) -> f64 {
    LEVELS
        .into_iter()
        .find(|&step| step > level + EPSILON)
        .unwrap_or(LEVELS[LEVELS.len() - 1])
}

/// The next smaller level, or the smallest one.
pub fn smaller(level: f64) -> f64 {
    LEVELS
        .into_iter()
        .rev()
        .find(|&step| step < level - EPSILON)
        .unwrap_or(LEVELS[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_up_and_down_from_the_default() {
        assert_eq!(larger(DEFAULT), 1.1);
        assert_eq!(smaller(DEFAULT), 0.9);
        assert_eq!(smaller(larger(DEFAULT)), DEFAULT);
    }

    #[test]
    fn stays_within_bounds() {
        assert_eq!(larger(3.0), 3.0);
        assert_eq!(larger(10.0), 3.0);
        assert_eq!(smaller(0.5), 0.5);
        assert_eq!(smaller(0.1), 0.5);
    }

    #[test]
    fn levels_between_steps_go_to_the_nearest_step() {
        assert_eq!(larger(1.05), 1.1);
        assert_eq!(smaller(1.05), 1.0);
        assert_eq!(larger(1.0999999), 1.25);
    }
}
