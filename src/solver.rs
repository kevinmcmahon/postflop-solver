use crate::interface::*;
use crate::mutex_like::*;
use crate::sliceop::*;
use crate::utility::*;
use std::io::{self, Write};
use std::mem::MaybeUninit;

#[cfg(feature = "custom-alloc")]
use crate::alloc::*;

/// When the cumulative strategy discount restarts during solving. Iterations are zero-based:
/// the first call to `solve_step` uses iteration 0. With a nonzero gamma, iteration 0 always
/// starts the cumulative-strategy discount from zero regardless of the schedule below, so read
/// `PowersOfFour`'s "1, 4, 16, ..." and `None`'s "never restarts" with that in mind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestartSchedule {
    /// Restarts at iterations 1, 4, 16, 64, 256, ... (the engine's long-standing behavior).
    PowersOfFour,
    /// Never restarts.
    None,
    /// Restarts counting from the largest listed iteration at or below the current one.
    /// `At(vec![1, 4, 16, 64, ...])` has the same effect as `PowersOfFour`.
    At(Vec<u32>),
}

/// Discounted CFR parameters. `alpha` and `beta` are the exponents applied to positive and
/// negative regrets, `gamma` the exponent applied to the cumulative strategy, and `restart`
/// decides when the cumulative strategy is zeroed.
#[derive(Debug, Clone, PartialEq)]
pub struct SolveParams {
    pub alpha: f64,
    pub beta: f64,
    pub gamma: u32,
    pub restart: RestartSchedule,
}

impl SolveParams {
    /// The engine's default: alpha 1.5, beta 0, gamma 3, restart at powers of four.
    pub fn current() -> Self {
        Self {
            alpha: 1.5,
            beta: 0.0,
            gamma: 3,
            restart: RestartSchedule::PowersOfFour,
        }
    }

    /// The Discounted CFR paper's recommendation: alpha 1.5, beta 0, gamma 2, no restart.
    pub fn paper() -> Self {
        Self {
            alpha: 1.5,
            beta: 0.0,
            gamma: 2,
            restart: RestartSchedule::None,
        }
    }
}

impl Default for SolveParams {
    fn default() -> Self {
        Self::current()
    }
}

/// Converts a discount term (`t` raised to the alpha or beta exponent) into the `[0, 1]`
/// discount factor `pow / (pow + 1.0)`. Floating point division makes `inf / (inf + 1.0)`
/// come out as `NaN`, even though the mathematical limit of `x / (x + 1)` as `x` approaches
/// infinity is exactly 1.0, so the infinite case is special-cased to that limit. `pow` reaches
/// infinity whenever `alpha` or `beta` is infinite and `t` is large enough: an infinite alpha
/// is the CFR+ setting, where positive regrets are never discounted, so this is a real input,
/// not an error case.
fn discount_factor(pow: f64) -> f32 {
    if pow.is_infinite() {
        1.0
    } else {
        (pow / (pow + 1.0)) as f32
    }
}

struct DiscountParams {
    alpha_t: f32,
    beta_t: f32,
    gamma_t: f32,
}

impl DiscountParams {
    fn new(current_iteration: u32, params: &SolveParams) -> Self {
        let restart_base = match &params.restart {
            // 0, 1, 4, 16, 64, 256, ...
            RestartSchedule::PowersOfFour => match current_iteration {
                0 => 0,
                x => 1 << ((x.leading_zeros() ^ 31) & !1),
            },
            RestartSchedule::None => 0,
            RestartSchedule::At(points) => points
                .iter()
                .copied()
                .filter(|&p| p <= current_iteration)
                .max()
                .unwrap_or(0),
        };

        let t_alpha = (current_iteration as i32 - 1).max(0) as f64;
        let t_gamma = (current_iteration - restart_base) as f64;

        // t * sqrt(t) is the expression the engine has always used for the 1.5 exponent; it is
        // not guaranteed to equal `t_alpha.powf(1.5)` bit for bit, so `SolveParams::current()`
        // keeps this exact form to reproduce old results bit for bit.
        let pow_alpha = if params.alpha == 1.5 {
            t_alpha * t_alpha.sqrt()
        } else {
            t_alpha.powf(params.alpha)
        };
        // No special case is needed for beta 0: `t.powf(0.0)` is exactly 1.0 for every `t`
        // (including 0.0), so the general formula already reduces to the historical 0.5.
        let pow_beta = t_alpha.powf(params.beta);
        // Saturate instead of casting directly: `powi` takes an `i32`, and a `gamma` above
        // `i32::MAX` would otherwise wrap into a negative exponent.
        let gamma_exponent = params.gamma.min(i32::MAX as u32) as i32;
        let pow_gamma = (t_gamma / (t_gamma + 1.0)).powi(gamma_exponent);

        Self {
            alpha_t: discount_factor(pow_alpha),
            beta_t: discount_factor(pow_beta),
            gamma_t: pow_gamma as f32,
        }
    }
}

/// Performs Discounted CFR algorithm until the given number of iterations or exploitability is
/// satisfied.
///
/// This method returns the exploitability of the obtained strategy. `params` selects the
/// discounting; `SolveParams::current()` is the engine's historical behavior.
///
/// # Panics
///
/// - The game is already solved.
/// - The game is not ready.
/// - `params.alpha` or `params.beta` is `NaN`.
pub fn solve_with_params<T: Game>(
    game: &mut T,
    max_num_iterations: u32,
    target_exploitability: f32,
    print_progress: bool,
    params: &SolveParams,
) -> f32 {
    if game.is_solved() {
        panic!("Game is already solved");
    }

    if !game.is_ready() {
        panic!("Game is not ready");
    }

    if params.alpha.is_nan() {
        panic!("SolveParams alpha must not be NaN");
    }

    if params.beta.is_nan() {
        panic!("SolveParams beta must not be NaN");
    }

    let mut root = game.root();
    let mut exploitability = compute_exploitability(game);

    if print_progress {
        print!("iteration: 0 / {max_num_iterations} ");
        print!("(exploitability = {exploitability:.4e})");
        io::stdout().flush().unwrap();
    }

    for t in 0..max_num_iterations {
        if exploitability <= target_exploitability {
            break;
        }

        let discount = DiscountParams::new(t, params);

        // alternating updates
        for player in 0..2 {
            let mut result = Vec::with_capacity(game.num_private_hands(player));
            solve_recursive(
                result.spare_capacity_mut(),
                game,
                &mut root,
                player,
                game.initial_weights(player ^ 1),
                &discount,
            );
        }

        if (t + 1) % 10 == 0 || t + 1 == max_num_iterations {
            exploitability = compute_exploitability(game);
        }

        if print_progress {
            print!("\riteration: {} / {} ", t + 1, max_num_iterations);
            print!("(exploitability = {exploitability:.4e})");
            io::stdout().flush().unwrap();
        }
    }

    if print_progress {
        println!();
        io::stdout().flush().unwrap();
    }

    finalize(game);

    exploitability
}

/// Performs Discounted CFR with `SolveParams::current()`. See `solve_with_params`.
///
/// # Panics
///
/// - The game is already solved.
/// - The game is not ready.
pub fn solve<T: Game>(
    game: &mut T,
    max_num_iterations: u32,
    target_exploitability: f32,
    print_progress: bool,
) -> f32 {
    solve_with_params(
        game,
        max_num_iterations,
        target_exploitability,
        print_progress,
        &SolveParams::current(),
    )
}

/// Proceeds Discounted CFR algorithm for one iteration. `params` selects the discounting;
/// `SolveParams::current()` is the engine's historical behavior.
///
/// # Panics
///
/// - The game is already solved.
/// - The game is not ready.
/// - `params.alpha` or `params.beta` is `NaN`.
#[inline]
pub fn solve_step_with_params<T: Game>(game: &T, current_iteration: u32, params: &SolveParams) {
    if game.is_solved() {
        panic!("Game is already solved");
    }

    if !game.is_ready() {
        panic!("Game is not ready");
    }

    if params.alpha.is_nan() {
        panic!("SolveParams alpha must not be NaN");
    }

    if params.beta.is_nan() {
        panic!("SolveParams beta must not be NaN");
    }

    let mut root = game.root();
    let discount = DiscountParams::new(current_iteration, params);

    // alternating updates
    for player in 0..2 {
        let mut result = Vec::with_capacity(game.num_private_hands(player));
        solve_recursive(
            result.spare_capacity_mut(),
            game,
            &mut root,
            player,
            game.initial_weights(player ^ 1),
            &discount,
        );
    }
}

/// Proceeds one iteration with `SolveParams::current()`. See `solve_step_with_params`.
///
/// # Panics
///
/// - The game is already solved.
/// - The game is not ready.
#[inline]
pub fn solve_step<T: Game>(game: &T, current_iteration: u32) {
    solve_step_with_params(game, current_iteration, &SolveParams::current())
}

/// Recursively solves the counterfactual values.
fn solve_recursive<T: Game>(
    result: &mut [MaybeUninit<f32>],
    game: &T,
    node: &mut T::Node,
    player: usize,
    cfreach: &[f32],
    params: &DiscountParams,
) {
    // return the counterfactual values when the `node` is terminal
    if node.is_terminal() {
        game.evaluate(result, node, player, cfreach);
        return;
    }

    let num_actions = node.num_actions();
    let num_hands = result.len();

    // simply recurse when the number of actions is one
    if num_actions == 1 && !node.is_chance() {
        let child = &mut node.play(0);
        solve_recursive(result, game, child, player, cfreach, params);
        return;
    }

    // allocate memory for storing the counterfactual values
    #[cfg(feature = "custom-alloc")]
    let cfv_actions = MutexLike::new(Vec::with_capacity_in(num_actions * num_hands, StackAlloc));
    #[cfg(not(feature = "custom-alloc"))]
    let cfv_actions = MutexLike::new(Vec::with_capacity(num_actions * num_hands));

    // if the `node` is chance
    if node.is_chance() {
        // update the reach probabilities
        #[cfg(feature = "custom-alloc")]
        let mut cfreach_updated = Vec::with_capacity_in(cfreach.len(), StackAlloc);
        #[cfg(not(feature = "custom-alloc"))]
        let mut cfreach_updated = Vec::with_capacity(cfreach.len());
        mul_slice_scalar_uninit(
            cfreach_updated.spare_capacity_mut(),
            cfreach,
            1.0 / game.chance_factor(node) as f32,
        );
        unsafe { cfreach_updated.set_len(cfreach.len()) };

        // compute the counterfactual values of each action
        for_each_child(node, |action| {
            solve_recursive(
                row_mut(cfv_actions.lock().spare_capacity_mut(), action, num_hands),
                game,
                &mut node.play(action),
                player,
                &cfreach_updated,
                params,
            );
        });

        // use 64-bit floating point values
        #[cfg(feature = "custom-alloc")]
        let mut result_f64 = Vec::with_capacity_in(num_hands, StackAlloc);
        #[cfg(not(feature = "custom-alloc"))]
        let mut result_f64 = Vec::with_capacity(num_hands);

        // sum up the counterfactual values
        let mut cfv_actions = cfv_actions.lock();
        unsafe { cfv_actions.set_len(num_actions * num_hands) };
        sum_slices_f64_uninit(result_f64.spare_capacity_mut(), &cfv_actions);
        unsafe { result_f64.set_len(num_hands) };

        // get information about isomorphic chances
        let isomorphic_chances = game.isomorphic_chances(node);

        // process isomorphic chances
        for (i, &isomorphic_index) in isomorphic_chances.iter().enumerate() {
            let swap_list = &game.isomorphic_swap(node, i)[player];
            let tmp = row_mut(&mut cfv_actions, isomorphic_index as usize, num_hands);

            apply_swap(tmp, swap_list);

            result_f64.iter_mut().zip(&*tmp).for_each(|(r, &v)| {
                *r += v as f64;
            });

            apply_swap(tmp, swap_list);
        }

        result.iter_mut().zip(&result_f64).for_each(|(r, &v)| {
            r.write(v as f32);
        });
    }
    // if the current player is `player`
    else if node.player() == player {
        // compute the counterfactual values of each action
        for_each_child(node, |action| {
            solve_recursive(
                row_mut(cfv_actions.lock().spare_capacity_mut(), action, num_hands),
                game,
                &mut node.play(action),
                player,
                cfreach,
                params,
            );
        });

        // compute the strategy by regret-maching algorithm
        let mut strategy = if game.is_compression_enabled() {
            regret_matching_compressed(node.regrets_compressed(), num_actions)
        } else {
            regret_matching(node.regrets(), num_actions)
        };

        // node-locking
        let locking = game.locking_strategy(node);
        apply_locking_strategy(&mut strategy, locking);

        // sum up the counterfactual values
        let mut cfv_actions = cfv_actions.lock();
        unsafe { cfv_actions.set_len(num_actions * num_hands) };
        let result = fma_slices_uninit(result, &strategy, &cfv_actions);

        if game.is_compression_enabled() {
            // update the cumulative strategy
            let scale = node.strategy_scale();
            let decoder = params.gamma_t * scale / u16::MAX as f32;
            let cum_strategy = node.strategy_compressed_mut();

            strategy.iter_mut().zip(&*cum_strategy).for_each(|(x, y)| {
                *x += (*y as f32) * decoder;
            });

            if !locking.is_empty() {
                strategy.iter_mut().zip(locking).for_each(|(d, s)| {
                    if s.is_sign_positive() {
                        *d = 0.0;
                    }
                })
            }

            let new_scale = encode_unsigned_slice(cum_strategy, &strategy);
            node.set_strategy_scale(new_scale);

            // update the cumulative regret
            let scale = node.regret_scale();
            let alpha_decoder = params.alpha_t * scale / i16::MAX as f32;
            let beta_decoder = params.beta_t * scale / i16::MAX as f32;
            let cum_regret = node.regrets_compressed_mut();

            cfv_actions.iter_mut().zip(&*cum_regret).for_each(|(x, y)| {
                *x += *y as f32 * if *y >= 0 { alpha_decoder } else { beta_decoder };
            });

            cfv_actions.chunks_exact_mut(num_hands).for_each(|row| {
                sub_slice(row, result);
            });

            if !locking.is_empty() {
                cfv_actions.iter_mut().zip(locking).for_each(|(d, s)| {
                    if s.is_sign_positive() {
                        *d = 0.0;
                    }
                })
            }

            let new_scale = encode_signed_slice(cum_regret, &cfv_actions);
            node.set_regret_scale(new_scale);
        } else {
            // update the cumulative strategy
            let gamma = params.gamma_t;
            let cum_strategy = node.strategy_mut();
            cum_strategy.iter_mut().zip(&strategy).for_each(|(x, y)| {
                *x = *x * gamma + *y;
            });

            // update the cumulative regret
            let (alpha, beta) = (params.alpha_t, params.beta_t);
            let cum_regret = node.regrets_mut();
            cum_regret.iter_mut().zip(&*cfv_actions).for_each(|(x, y)| {
                let coef = if x.is_sign_positive() { alpha } else { beta };
                *x = *x * coef + *y;
            });
            cum_regret.chunks_exact_mut(num_hands).for_each(|row| {
                sub_slice(row, result);
            });
        }
    }
    // if the current player is not `player`
    else {
        // compute the strategy by regret-matching algorithm
        let mut cfreach_actions = if game.is_compression_enabled() {
            regret_matching_compressed(node.regrets_compressed(), num_actions)
        } else {
            regret_matching(node.regrets(), num_actions)
        };

        // node-locking
        let locking = game.locking_strategy(node);
        apply_locking_strategy(&mut cfreach_actions, locking);

        // update the reach probabilities
        let row_size = cfreach.len();
        cfreach_actions.chunks_exact_mut(row_size).for_each(|row| {
            mul_slice(row, cfreach);
        });

        // compute the counterfactual values of each action
        for_each_child(node, |action| {
            solve_recursive(
                row_mut(cfv_actions.lock().spare_capacity_mut(), action, num_hands),
                game,
                &mut node.play(action),
                player,
                row(&cfreach_actions, action, row_size),
                params,
            );
        });

        // sum up the counterfactual values
        let mut cfv_actions = cfv_actions.lock();
        unsafe { cfv_actions.set_len(num_actions * num_hands) };
        sum_slices_uninit(result, &cfv_actions);
    }
}

/// Computes the strategy by regret-matching algorithm.
#[cfg(feature = "custom-alloc")]
#[inline]
fn regret_matching(regret: &[f32], num_actions: usize) -> Vec<f32, StackAlloc> {
    let mut strategy = Vec::with_capacity_in(regret.len(), StackAlloc);
    let uninit = strategy.spare_capacity_mut();
    uninit.iter_mut().zip(regret).for_each(|(s, r)| {
        s.write(max(*r, 0.0));
    });
    unsafe { strategy.set_len(regret.len()) };

    let row_size = regret.len() / num_actions;
    let mut denom = Vec::with_capacity_in(row_size, StackAlloc);
    sum_slices_uninit(denom.spare_capacity_mut(), &strategy);
    unsafe { denom.set_len(row_size) };

    let default = 1.0 / num_actions as f32;
    strategy.chunks_exact_mut(row_size).for_each(|row| {
        div_slice(row, &denom, default);
    });

    strategy
}

/// Computes the strategy by regret-matching algorithm.
#[cfg(not(feature = "custom-alloc"))]
#[inline]
fn regret_matching(regret: &[f32], num_actions: usize) -> Vec<f32> {
    let mut strategy = Vec::with_capacity(regret.len());
    let uninit = strategy.spare_capacity_mut();
    uninit.iter_mut().zip(regret).for_each(|(s, r)| {
        s.write(max(*r, 0.0));
    });
    unsafe { strategy.set_len(regret.len()) };

    let row_size = regret.len() / num_actions;
    let mut denom = Vec::with_capacity(row_size);
    sum_slices_uninit(denom.spare_capacity_mut(), &strategy);
    unsafe { denom.set_len(row_size) };

    let default = 1.0 / num_actions as f32;
    strategy.chunks_exact_mut(row_size).for_each(|row| {
        div_slice(row, &denom, default);
    });

    strategy
}

/// Computes the strategy by regret-matching algorithm.
#[cfg(feature = "custom-alloc")]
#[inline]
fn regret_matching_compressed(regret: &[i16], num_actions: usize) -> Vec<f32, StackAlloc> {
    let mut strategy = Vec::with_capacity_in(regret.len(), StackAlloc);
    strategy.extend(regret.iter().map(|&r| r.max(0) as f32));

    let row_size = strategy.len() / num_actions;
    let mut denom = Vec::with_capacity_in(row_size, StackAlloc);
    sum_slices_uninit(denom.spare_capacity_mut(), &strategy);
    unsafe { denom.set_len(row_size) };

    let default = 1.0 / num_actions as f32;
    strategy.chunks_exact_mut(row_size).for_each(|row| {
        div_slice(row, &denom, default);
    });

    strategy
}

/// Computes the strategy by regret-matching algorithm.
#[cfg(not(feature = "custom-alloc"))]
#[inline]
fn regret_matching_compressed(regret: &[i16], num_actions: usize) -> Vec<f32> {
    let mut strategy = Vec::with_capacity(regret.len());
    strategy.extend(regret.iter().map(|&r| r.max(0) as f32));

    let row_size = strategy.len() / num_actions;
    let mut denom = Vec::with_capacity(row_size);
    sum_slices_uninit(denom.spare_capacity_mut(), &strategy);
    unsafe { denom.set_len(row_size) };

    let default = 1.0 / num_actions as f32;
    strategy.chunks_exact_mut(row_size).for_each(|row| {
        div_slice(row, &denom, default);
    });

    strategy
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The discounting the engine used before `SolveParams` existed, kept verbatim as the oracle.
    fn legacy(current_iteration: u32) -> (f32, f32, f32) {
        let nearest_lower_power_of_4 = match current_iteration {
            0 => 0,
            x => 1 << ((x.leading_zeros() ^ 31) & !1),
        };
        let t_alpha = (current_iteration as i32 - 1).max(0) as f64;
        let t_gamma = (current_iteration - nearest_lower_power_of_4) as f64;
        let pow_alpha = t_alpha * t_alpha.sqrt();
        let pow_gamma = (t_gamma / (t_gamma + 1.0)).powi(3);
        (
            (pow_alpha / (pow_alpha + 1.0)) as f32,
            0.5,
            pow_gamma as f32,
        )
    }

    #[test]
    fn current_preset_matches_legacy_discounting_bit_for_bit() {
        let params = SolveParams::current();
        for t in 0..5000 {
            let d = DiscountParams::new(t, &params);
            let (a, b, g) = legacy(t);
            assert_eq!(d.alpha_t.to_bits(), a.to_bits(), "alpha at {t}");
            assert_eq!(d.beta_t.to_bits(), b.to_bits(), "beta at {t}");
            assert_eq!(d.gamma_t.to_bits(), g.to_bits(), "gamma at {t}");
        }
    }

    #[test]
    fn paper_preset_never_restarts_and_uses_square() {
        let params = SolveParams::paper();
        let d = DiscountParams::new(16, &params);
        let t = 16.0f64;
        assert_eq!(d.gamma_t, ((t / (t + 1.0)).powi(2)) as f32);
        assert!(DiscountParams::new(64, &params).gamma_t > 0.9);
    }

    #[test]
    fn explicit_restart_list_resets_at_listed_iterations() {
        let params = SolveParams {
            restart: RestartSchedule::At(vec![100, 300]),
            ..SolveParams::current()
        };
        assert_eq!(DiscountParams::new(100, &params).gamma_t, 0.0);
        assert_eq!(DiscountParams::new(300, &params).gamma_t, 0.0);
        assert!(DiscountParams::new(200, &params).gamma_t > 0.0);
        assert_eq!(DiscountParams::new(0, &params).gamma_t, 0.0);
    }

    /// An explicit list of every power of four up to 1024 covers all restarts that
    /// `PowersOfFour` triggers below iteration 4096, so the two schedules must agree there.
    #[test]
    fn explicit_powers_of_four_list_matches_powers_of_four_schedule() {
        let powers_of_four = SolveParams::current();
        let explicit = SolveParams {
            restart: RestartSchedule::At(vec![1, 4, 16, 64, 256, 1024]),
            ..SolveParams::current()
        };
        for t in 0..4096 {
            let a = DiscountParams::new(t, &powers_of_four);
            let b = DiscountParams::new(t, &explicit);
            assert_eq!(a.alpha_t.to_bits(), b.alpha_t.to_bits(), "alpha at {t}");
            assert_eq!(a.beta_t.to_bits(), b.beta_t.to_bits(), "beta at {t}");
            assert_eq!(a.gamma_t.to_bits(), b.gamma_t.to_bits(), "gamma at {t}");
        }
    }

    /// `t_alpha` (`max(current_iteration - 1, 0)`) is 0 at iterations 0 and 1, exactly 1 at
    /// iteration 2, and at least 2 from iteration 3 on. An infinite alpha is the CFR+ setting:
    /// `0^inf = 0`, `1^inf = 1` (a fixed point independent of the exponent), and `x^inf = inf`
    /// for `x > 1`, which `discount_factor` maps to its limit of 1.0 instead of `NaN`.
    #[test]
    fn infinite_alpha_reaches_the_cfr_plus_limit_without_nan() {
        let params = SolveParams {
            alpha: f64::INFINITY,
            ..SolveParams::current()
        };
        for t in 0..100 {
            let alpha_t = DiscountParams::new(t, &params).alpha_t;
            assert!(!alpha_t.is_nan(), "alpha_t at t={t} must not be NaN");
            assert!(
                !alpha_t.is_infinite(),
                "alpha_t at t={t} must not be infinite"
            );
            let expected = match t {
                0 | 1 => 0.0,
                2 => 0.5,
                _ => 1.0,
            };
            assert_eq!(alpha_t, expected, "alpha_t at t={t}");
        }
    }

    /// The mirror image of `infinite_alpha_reaches_the_cfr_plus_limit_without_nan`: `0^-inf =
    /// inf`, `1^-inf = 1` (the same fixed point), and `x^-inf = 0` for `x > 1`.
    #[test]
    fn negative_infinite_beta_reaches_zero_without_nan() {
        let params = SolveParams {
            beta: f64::NEG_INFINITY,
            ..SolveParams::current()
        };
        for t in 0..100 {
            let beta_t = DiscountParams::new(t, &params).beta_t;
            assert!(!beta_t.is_nan(), "beta_t at t={t} must not be NaN");
            assert!(
                !beta_t.is_infinite(),
                "beta_t at t={t} must not be infinite"
            );
            let expected = match t {
                0 | 1 => 1.0,
                2 => 0.5,
                _ => 0.0,
            };
            assert_eq!(beta_t, expected, "beta_t at t={t}");
        }
    }

    /// A negative exponent applied to a base of 0 (`t_alpha` at iterations 0 and 1) is
    /// infinity, not NaN, and `discount_factor` maps that to its limit of 1.0.
    #[test]
    fn negative_alpha_never_produces_nan() {
        let params = SolveParams {
            alpha: -1.0,
            ..SolveParams::current()
        };
        assert_eq!(DiscountParams::new(0, &params).alpha_t, 1.0);
        assert_eq!(DiscountParams::new(1, &params).alpha_t, 1.0);
        for t in 0..1000 {
            let alpha_t = DiscountParams::new(t, &params).alpha_t;
            assert!(!alpha_t.is_nan(), "alpha_t at t={t} must not be NaN");
        }
    }

    #[test]
    fn gamma_saturates_instead_of_wrapping() {
        let params = SolveParams {
            gamma: u32::MAX,
            ..SolveParams::current()
        };
        for t in [0, 1, 2, 3, 100, 100_000, u32::MAX] {
            let gamma_t = DiscountParams::new(t, &params).gamma_t;
            assert!(
                !gamma_t.is_infinite(),
                "gamma_t at t={t} must not be infinite"
            );
            assert!(
                (0.0..=1.0).contains(&gamma_t),
                "gamma_t at t={t} = {gamma_t} is out of [0, 1]"
            );
        }
    }

    #[test]
    fn empty_at_schedule_matches_none_bit_for_bit() {
        let empty = SolveParams {
            restart: RestartSchedule::At(vec![]),
            ..SolveParams::current()
        };
        let none = SolveParams {
            restart: RestartSchedule::None,
            ..SolveParams::current()
        };
        for t in 0..300 {
            let a = DiscountParams::new(t, &empty);
            let b = DiscountParams::new(t, &none);
            assert_eq!(a.alpha_t.to_bits(), b.alpha_t.to_bits(), "alpha at {t}");
            assert_eq!(a.beta_t.to_bits(), b.beta_t.to_bits(), "beta at {t}");
            assert_eq!(a.gamma_t.to_bits(), b.gamma_t.to_bits(), "gamma at {t}");
        }
    }

    #[test]
    fn unsorted_at_schedule_matches_sorted_bit_for_bit() {
        let unsorted = SolveParams {
            restart: RestartSchedule::At(vec![64, 4, 16]),
            ..SolveParams::current()
        };
        let sorted = SolveParams {
            restart: RestartSchedule::At(vec![4, 16, 64]),
            ..SolveParams::current()
        };
        for t in 0..300 {
            let a = DiscountParams::new(t, &unsorted);
            let b = DiscountParams::new(t, &sorted);
            assert_eq!(a.alpha_t.to_bits(), b.alpha_t.to_bits(), "alpha at {t}");
            assert_eq!(a.beta_t.to_bits(), b.beta_t.to_bits(), "beta at {t}");
            assert_eq!(a.gamma_t.to_bits(), b.gamma_t.to_bits(), "gamma at {t}");
        }
    }
}
