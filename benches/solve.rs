//! Throughput benchmarks for the solver.
//!
//! Each benchmark builds one game, then repeatedly allocates its storage and runs a fixed number of
//! Discounted CFR iterations. The measured quantity is the wall time of one such solve, so a change
//! to the layout of the node storage or to the inner kernels shows up as a change in the reported
//! time. `docs/benchmarks.md` records the baseline numbers and the rule for accepting a refactor.
//!
//! The primary numbers are taken with `RAYON_NUM_THREADS=1`. Thread scheduling on a machine with
//! both performance and efficiency cores adds far more variance than the effects worth catching.

use criterion::{Criterion, SamplingMode, criterion_group, criterion_main};
use postflop_solver::{PostFlopGame, solve};
use std::time::Duration;

mod scenarios;

/// Number of samples criterion collects per scenario. Ten is its minimum, which is what a solve of
/// several seconds can afford.
const SAMPLE_SIZE: usize = 10;

/// Warm-up budget. One solve already exceeds it, so warm-up costs a single extra solve and gives
/// criterion the estimate it uses to plan the measurement.
const WARM_UP_TIME: Duration = Duration::from_secs(3);

/// Measurement budget per scenario. Criterion divides it by [`SAMPLE_SIZE`] and rounds up to a
/// whole number of solves per sample, and it warns whenever that number comes out as one. With
/// every scenario tuned to between 5 and 6 s per solve on a single thread, 100 s puts two solves in
/// each sample and keeps the run quiet.
const MEASUREMENT_TIME: Duration = Duration::from_secs(100);

/// CFR iterations for the flop scenario that stores node values as 32-bit floats. One solve takes
/// about 5.5 s on a single thread.
const FLOP_UNCOMPRESSED_ITERATIONS: u32 = 10;

/// CFR iterations for the flop scenario that stores node values as 16-bit integers. One solve takes
/// about 5.8 s on a single thread.
const FLOP_COMPRESSED_ITERATIONS: u32 = 10;

/// CFR iterations for the turn scenario. One solve takes about 6.0 s on a single thread.
const TURN_ITERATIONS: u32 = 750;

/// CFR iterations for the river scenario. The tree is small enough that a single iteration costs
/// tens of microseconds, so it takes a large count to reach a measurable solve. One solve takes
/// about 5.6 s on a single thread.
const RIVER_ITERATIONS: u32 = 250_000;

/// CFR iterations for the bunching scenario. One solve takes about 5.9 s on a single thread.
const BUNCHING_ITERATIONS: u32 = 50;

/// One benchmark case. The game is built by `build` once, outside the measured closure.
struct Scenario {
    name: &'static str,
    build: fn() -> PostFlopGame,
    compressed: bool,
    iterations: u32,
}

const SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "flop_uncompressed",
        build: scenarios::flop_game,
        compressed: false,
        iterations: FLOP_UNCOMPRESSED_ITERATIONS,
    },
    Scenario {
        name: "flop_compressed",
        build: scenarios::flop_game,
        compressed: true,
        iterations: FLOP_COMPRESSED_ITERATIONS,
    },
    Scenario {
        name: "turn_start",
        build: scenarios::turn_game,
        compressed: false,
        iterations: TURN_ITERATIONS,
    },
    Scenario {
        name: "river_start",
        build: scenarios::river_game,
        compressed: false,
        iterations: RIVER_ITERATIONS,
    },
    Scenario {
        name: "bunching",
        build: scenarios::bunching_game,
        compressed: false,
        iterations: BUNCHING_ITERATIONS,
    },
];

fn solve_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("solve");
    group.sample_size(SAMPLE_SIZE);
    group.warm_up_time(WARM_UP_TIME);
    group.measurement_time(MEASUREMENT_TIME);
    group.sampling_mode(SamplingMode::Flat);

    for scenario in SCENARIOS {
        // Criterion calls the closure below only for benchmarks that pass its name filter, so
        // building the game there keeps a run that names one scenario from paying for the others.
        // The first call builds it and the remaining calls reuse it; none of this is timed.
        let mut game = None;

        group.bench_function(scenario.name, |b| {
            let game = game.get_or_insert_with(scenario.build);

            b.iter(|| {
                // `allocate_memory` reallocates the storage and returns the game to the unsolved
                // state, which is what lets the same game be solved once per iteration. It accounts
                // for at most 0.13 percent of a solve, so it stays inside the measurement rather
                // than in a per-iteration setup step.
                game.allocate_memory(scenario.compressed);
                solve(game, scenario.iterations, 0.0, false)
            })
        });
    }

    group.finish();
}

criterion_group!(benches, solve_throughput);
criterion_main!(benches);
