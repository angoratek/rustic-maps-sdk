//! Interpolation curves for the `interpolate` expression.
//!
//! Inputs are the numeric stop positions; outputs are numbers or colours.
//! The exponential curve is the specification's
//! `(base^(z - z0) - 1) / (base^(z1 - z0) - 1)`; a base of exactly one is
//! linear. The cubic-bezier curve solves its x component for the normalized
//! position and evaluates its y component, so a style can bend the response
//! between two stops without changing the stop values.

/// The number of bisection steps used to invert a cubic-bezier curve.
const CUBIC_BEZIER_SOLVE_ITERATIONS: u32 = 32;
/// The maximum distance between a solved cubic-bezier x and the target x.
const CUBIC_BEZIER_SOLVE_TOLERANCE: f64 = 1.0e-6;
/// The exponent that makes an exponential curve linear.
const LINEAR_BASE: f64 = 1.0;
/// The first power of the cubic-bezier polynomial.
const CUBIC_FIRST_POWER: f64 = 3.0;
/// The second power of the cubic-bezier polynomial.
const CUBIC_SECOND_POWER: f64 = 3.0;

/// The curve used between two `interpolate` stops.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Interpolation {
    /// Linear interpolation.
    Linear,
    /// Exponential interpolation with the given base.
    Exponential {
        /// The curve's base; `1.0` is linear.
        base: f64,
    },
    /// A cubic-bezier easing curve.
    CubicBezier {
        /// The first control point's x.
        x1: f64,
        /// The first control point's y.
        y1: f64,
        /// The second control point's x.
        x2: f64,
        /// The second control point's y.
        y2: f64,
    },
}

impl Interpolation {
    /// The fraction of the way from `lower` to `upper` at `input`.
    ///
    /// The caller guarantees `lower < upper`; `input` is clamped to the stop
    /// range before this is called.
    pub fn fraction(self, lower: f64, upper: f64, input: f64) -> f64 {
        let span = upper - lower;
        if span <= 0.0 {
            return 0.0;
        }
        match self {
            Self::Linear => (input - lower) / span,
            Self::Exponential { base } => {
                if (base - LINEAR_BASE).abs() <= f64::EPSILON {
                    (input - lower) / span
                } else {
                    (base.powf(input - lower) - 1.0) / (base.powf(span) - 1.0)
                }
            }
            Self::CubicBezier { x1, y1, x2, y2 } => {
                let target = (input - lower) / span;
                let parameter = solve_cubic_bezier_parameter(target, x1, x2);
                cubic_bezier_value(parameter, y1, y2)
            }
        }
    }
}

/// Solves `bezier_x(t) = target` for `t` in `0.0..=1.0` by bisection.
fn solve_cubic_bezier_parameter(target: f64, x1: f64, x2: f64) -> f64 {
    // The curve passes through its endpoints exactly. Handling them first
    // avoids the bisection's residual error at `x = 1`, where the y
    // derivative can be much larger than the x tolerance.
    if target <= 0.0 {
        return 0.0;
    }
    if target >= 1.0 {
        return 1.0;
    }
    let mut lower = 0.0;
    let mut upper = 1.0;
    for _ in 0..CUBIC_BEZIER_SOLVE_ITERATIONS {
        let middle = (lower + upper) * 0.5;
        let value = cubic_bezier_value(middle, x1, x2);
        if (value - target).abs() <= CUBIC_BEZIER_SOLVE_TOLERANCE {
            return middle;
        }
        if value < target {
            lower = middle;
        } else {
            upper = middle;
        }
    }
    (lower + upper) * 0.5
}

/// Evaluates one component of a cubic-bezier curve at `parameter`.
fn cubic_bezier_value(parameter: f64, first_control: f64, second_control: f64) -> f64 {
    let complement = 1.0 - parameter;
    CUBIC_FIRST_POWER * complement * complement * parameter * first_control
        + CUBIC_SECOND_POWER * complement * parameter * parameter * second_control
        + parameter * parameter * parameter
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_uses_the_fraction_directly() {
        assert!((Interpolation::Linear.fraction(0.0, 10.0, 2.5) - 0.25).abs() < 1e-12);
    }

    #[test]
    fn exponential_base_one_is_linear() {
        let curve = Interpolation::Exponential { base: 1.0 };
        assert!((curve.fraction(0.0, 10.0, 2.5) - 0.25).abs() < 1e-12);
    }

    #[test]
    fn exponential_base_two_matches_the_specification_formula() {
        let curve = Interpolation::Exponential { base: 2.0 };
        let expected = (2.0_f64.powf(2.0) - 1.0) / (2.0_f64.powf(10.0) - 1.0);
        assert!((curve.fraction(0.0, 10.0, 2.0) - expected).abs() < 1e-12);
    }

    #[test]
    fn cubic_bezier_solves_its_x_curve() {
        let curve = Interpolation::CubicBezier {
            x1: 0.25,
            y1: 0.1,
            x2: 0.25,
            y2: 1.0,
        };
        assert!((curve.fraction(0.0, 1.0, 0.0) - 0.0).abs() < 1e-6);
        assert!((curve.fraction(0.0, 1.0, 1.0) - 1.0).abs() < 1e-6);
        let middle = curve.fraction(0.0, 1.0, 0.5);
        assert!(middle > 0.0 && middle < 1.0);
    }
}
