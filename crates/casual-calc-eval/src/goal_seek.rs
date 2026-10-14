//! Goal Seek — Excel's Data ▸ What-If Analysis ▸ Goal Seek (XL-FEAT-032).
//!
//! Find the input `x` for which `f(x)` reaches a target. The function is a
//! black box: the caller evaluates the workbook with the changing cell set to
//! `x` and reads the target cell back, so this module knows nothing about
//! cells and can be tested on plain closures.

/// Excel's iteration settings (File ▸ Options ▸ Formulas): at most 100
/// iterations, and a result counts once it is within 0.001 of the target.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GoalSeekOptions {
    /// The most evaluations of `f` the search may make.
    pub max_iterations: u32,
    /// How close `f(x)` must come to the target to count as found.
    pub max_change: f64,
}

impl Default for GoalSeekOptions {
    fn default() -> Self {
        Self {
            max_iterations: 100,
            max_change: 0.001,
        }
    }
}

/// What a search ended with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GoalSeekResult {
    /// Whether `value` is within `max_change` of the target.
    pub found: bool,
    /// The input tried last that came closest to the target.
    pub input: f64,
    /// `f(input)` — the target cell's value at that input.
    pub value: f64,
    /// How many times `f` was evaluated.
    pub iterations: u32,
}

/// Search for `x` with `f(x) == target`, starting from `start`.
///
/// `f` returns `None` where the target is not a number (an error value, text),
/// and the search steps back from such an input.
pub fn goal_seek(
    mut f: impl FnMut(f64) -> Option<f64>,
    target: f64,
    start: f64,
    opts: GoalSeekOptions,
) -> GoalSeekResult {
    let mut p = Probe {
        f: &mut f,
        target,
        cap: opts.max_iterations.max(1),
        tol: opts.max_change.abs(),
        iterations: 0,
        best: None,
        hit: None,
    };
    let x0 = if start.is_finite() { start } else { 0.0 };
    // The first step's size: one percent of the input, or 0.01 from zero.
    let h = if x0 == 0.0 { 0.01 } else { x0.abs() * 0.01 };

    // A first point with a number at it. The start usually is one; if it is
    // not (the formula errors there), walk outward until one is.
    let mut a = None;
    let mut k = 0;
    while a.is_none() && !p.spent() {
        let x = if k == 0 {
            x0
        } else {
            let step = h * f64::powi(2.0, (k + 1) / 2);
            if k % 2 == 1 { x0 + step } else { x0 - step }
        };
        k += 1;
        if let Some(g) = p.at(x) {
            a = Some((x, g));
        }
    }
    let Some((mut xa, mut ga)) = a else {
        return p.result(start);
    };
    if p.hit.is_some() {
        return p.result(start);
    }

    // Secant steps until the target is crossed, then a bracketing search
    // (Illinois regula falsi, falling back to bisection) inside the crossing.
    let (mut xb, mut gb) = (xa, ga);
    let mut have_b = false;
    let mut bracket: Option<(f64, f64, f64, f64)> = None;
    while !p.spent() && p.hit.is_none() {
        if let Some((lo, glo, hi, ghi)) = bracket {
            if (hi - lo).abs() <= 4.0 * f64::EPSILON * hi.abs().max(lo.abs()).max(f64::MIN_POSITIVE)
            {
                // The crossing has closed to adjacent numbers without the
                // target being met: a jump, not a root.
                break;
            }
            let mut x = hi - ghi * (hi - lo) / (ghi - glo);
            let (min, max) = (lo.min(hi), lo.max(hi));
            if !(x > min && x < max) {
                x = 0.5 * (lo + hi);
            }
            let Some(g) = p.at(x) else {
                // No number inside the crossing: the target is not continuous
                // here, so there is nothing a smaller step would find.
                break;
            };
            bracket = Some(if g.signum() == ghi.signum() {
                // The same end moved twice: halve the other's weight (Illinois),
                // which is what keeps regula falsi from crawling.
                (lo, 0.5 * glo, x, g)
            } else {
                (hi, ghi, x, g)
            });
            continue;
        }

        let x = if !have_b {
            xa + h
        } else {
            let slope = (gb - ga) / (xb - xa);
            let x = xb - gb / slope;
            if slope == 0.0 || !x.is_finite() {
                // Flat here: keep going the way we came, twice as far.
                xb + 2.0 * (xb - xa)
            } else {
                x
            }
        };
        if !x.is_finite() {
            break;
        }
        let mut x = x;
        let g = loop {
            if let Some(g) = p.at(x) {
                break Some(g);
            }
            if p.spent() {
                break None;
            }
            // An error at the trial input: step back halfway towards the last
            // input that had a number.
            x = 0.5 * (x + xb);
        };
        let Some(g) = g else { break };
        if have_b {
            (xa, ga) = (xb, gb);
        }
        (xb, gb) = (x, g);
        have_b = true;
        if ga.signum() != gb.signum() {
            bracket = Some((xa, ga, xb, gb));
        }
    }
    p.result(start)
}

/// The evaluations a search has made, and the closest it has come.
struct Probe<'a, F: FnMut(f64) -> Option<f64>> {
    f: &'a mut F,
    target: f64,
    cap: u32,
    tol: f64,
    iterations: u32,
    /// `(input, value)` with the smallest miss so far.
    best: Option<(f64, f64)>,
    /// `(input, value)` within tolerance, once one is seen.
    hit: Option<(f64, f64)>,
}

impl<F: FnMut(f64) -> Option<f64>> Probe<'_, F> {
    fn spent(&self) -> bool {
        self.iterations >= self.cap
    }

    /// `f(x) - target`, or `None` where `f` has no number.
    fn at(&mut self, x: f64) -> Option<f64> {
        self.iterations += 1;
        let v = (self.f)(x).filter(|v| v.is_finite())?;
        let miss = (v - self.target).abs();
        if self
            .best
            .is_none_or(|(_, bv)| miss < (bv - self.target).abs())
        {
            self.best = Some((x, v));
        }
        if miss <= self.tol && self.hit.is_none() {
            self.hit = Some((x, v));
        }
        Some(v - self.target)
    }

    fn result(&self, start: f64) -> GoalSeekResult {
        let (found, (input, value)) = match (self.hit, self.best) {
            (Some(h), _) => (true, h),
            (None, Some(b)) => (false, b),
            (None, None) => (false, (start, f64::NAN)),
        };
        GoalSeekResult {
            found,
            input,
            value,
            iterations: self.iterations,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{GoalSeekOptions, goal_seek};

    fn seek(f: impl FnMut(f64) -> Option<f64>, target: f64, start: f64) -> super::GoalSeekResult {
        goal_seek(f, target, start, GoalSeekOptions::default())
    }

    #[test]
    fn a_linear_target_is_found_at_once() {
        // =A1*3+4 to 19 → A1 = 5.
        let r = seek(|x| Some(x * 3.0 + 4.0), 19.0, 0.0);
        assert!(r.found, "{r:?}");
        assert!((r.input - 5.0).abs() < 1e-6, "{r:?}");
        assert!((r.value - 19.0).abs() <= 0.001);
        assert!(
            r.iterations <= 5,
            "a straight line needs a couple of steps, took {r:?}"
        );
    }

    #[test]
    fn a_square_reaches_root_two() {
        // =A1^2 to 2 from A1 = 1 → √2.
        let r = seek(|x| Some(x * x), 2.0, 1.0);
        assert!(r.found, "{r:?}");
        assert!((r.input - std::f64::consts::SQRT_2).abs() < 1e-3, "{r:?}");
        assert!((r.value - 2.0).abs() <= 0.001);
    }

    #[test]
    fn a_square_from_zero_still_moves() {
        // The derivative is zero at the start; the search must not stall there.
        let r = seek(|x| Some(x * x), 9.0, 0.0);
        assert!(r.found, "{r:?}");
        assert!((r.input.abs() - 3.0).abs() < 1e-3, "{r:?}");
    }

    #[test]
    fn a_non_monotone_target_finds_a_root() {
        // sin(x) to 0.5 from 0: the nearest root is π/6, another at 5π/6.
        let r = seek(|x| Some(x.sin()), 0.5, 0.0);
        assert!(r.found, "{r:?}");
        assert!((r.input.sin() - 0.5).abs() <= 0.001, "{r:?}");
        // A parabola whose slope is downhill at the start: (x-3)^2 to 1 from 0.
        let r = seek(|x| Some((x - 3.0) * (x - 3.0)), 1.0, 0.0);
        assert!(r.found, "{r:?}");
        assert!(
            (r.input - 2.0).abs() < 1e-3 || (r.input - 4.0).abs() < 1e-3,
            "{r:?}"
        );
    }

    #[test]
    fn steps_back_from_an_error_value() {
        // =SQRT(A1) to 0.1 from A1 = 4: the first secant step lands below
        // zero, where SQRT gives #NUM!; the search has to come back.
        let mut errored = false;
        let r = seek(
            |x| {
                errored |= x < 0.0;
                (x >= 0.0).then(|| x.sqrt())
            },
            0.1,
            4.0,
        );
        assert!(
            errored,
            "the case is meant to try an input with an error value"
        );
        assert!(r.found, "{r:?}");
        assert!((r.input - 0.01).abs() < 0.001, "{r:?}");
    }

    #[test]
    fn no_solution_reports_the_closest_value() {
        // =A1^2+1 to 0 has no real solution; the closest is 1 at A1 = 0.
        let r = seek(|x| Some(x * x + 1.0), 0.0, 2.0);
        assert!(!r.found, "{r:?}");
        assert!(r.value.is_finite());
        assert!(r.value < 1.1, "should end near the minimum, got {r:?}");
        assert!(r.iterations <= 100);
    }

    #[test]
    fn the_iteration_cap_is_kept() {
        // A step with no crossing value: a bracket exists but no root.
        let mut calls = 0u32;
        let r = goal_seek(
            |x| {
                calls += 1;
                Some(if x < 1.0 { 0.0 } else { 10.0 })
            },
            5.0,
            0.0,
            GoalSeekOptions {
                max_iterations: 7,
                max_change: 0.001,
            },
        );
        assert!(!r.found, "{r:?}");
        // It spends the whole budget looking, and not one evaluation more.
        assert_eq!(calls, 7, "evaluated {calls} times under a cap of 7");
        assert_eq!(r.iterations, calls);
        // Never a solution, whatever the cap.
        let r = seek(|x| Some(if x < 1.0 { 0.0 } else { 10.0 }), 5.0, 0.0);
        assert!(!r.found && r.iterations <= 100, "{r:?}");
    }

    #[test]
    fn already_there_costs_one_evaluation() {
        let r = seek(|x| Some(x + 1.0), 3.0, 2.0);
        assert!(r.found);
        assert_eq!(r.input, 2.0);
        assert_eq!(r.iterations, 1);
    }

    #[test]
    fn a_target_that_is_never_a_number_is_not_found() {
        let r = seek(|_| None, 3.0, 2.0);
        assert!(!r.found);
        assert!(r.value.is_nan(), "no value was ever read: {r:?}");
        // It looked — outward from the start — and stopped at the cap.
        assert_eq!(r.iterations, 100);
    }
}
