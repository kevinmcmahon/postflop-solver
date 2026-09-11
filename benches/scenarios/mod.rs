//! Game configurations shared by the solver benchmarks.
//!
//! The configurations match the scenarios of the golden tests in `tests/golden.rs`, so a change
//! that the golden tests prove behaviour preserving is measured here on the same games. Integration
//! tests and benchmarks are separate crates and cannot share code, so the configuration literals
//! appear in both places.
//!
//! Cargo makes a bench target out of every `benches/*.rs` file, so this module lives one directory
//! down. That keeps it a module of the `solve` bench instead of a target of its own that would run
//! no benchmarks.

use postflop_solver::*;

const OOP_RANGE: &str = "66+,A8s+,A5s-A4s,AJo+,K9s+,KQo,QTs+,JTs,96s+,85s+,75s+,65s,54s";
const IP_RANGE: &str = "QQ-22,AQs-A2s,ATo+,K5s+,KJo+,Q8s+,J8s+,T7s+,96s+,86s+,75s+,64s+,53s+";

/// The ranges of `examples/basic.rs` on the given board. A turn or river of `None` is not dealt.
fn card_config(flop: &str, turn: Option<&str>, river: Option<&str>) -> CardConfig {
    CardConfig {
        range: [OOP_RANGE.parse().unwrap(), IP_RANGE.parse().unwrap()],
        flop: flop_from_str(flop).unwrap(),
        turn: turn.map_or(NOT_DEALT, |card| card_from_str(card).unwrap()),
        river: river.map_or(NOT_DEALT, |card| card_from_str(card).unwrap()),
    }
}

/// The bet sizes and thresholds of `examples/basic.rs`, started from the given street.
fn tree_config(initial_state: BoardState) -> TreeConfig {
    let bet_sizes = BetSizeOptions::try_from(("60%, e, a", "2.5x")).unwrap();

    TreeConfig {
        initial_state,
        starting_pot: 200,
        effective_stack: 900,
        rake_rate: 0.0,
        rake_cap: 0.0,
        flop_bet_sizes: [bet_sizes.clone(), bet_sizes.clone()],
        turn_bet_sizes: [bet_sizes.clone(), bet_sizes.clone()],
        river_bet_sizes: [bet_sizes.clone(), bet_sizes],
        turn_donk_sizes: None,
        river_donk_sizes: Some(DonkSizeOptions::try_from("50%").unwrap()),
        add_allin_threshold: 1.5,
        force_allin_threshold: 0.15,
        merging_threshold: 0.1,
    }
}

fn game(card_config: CardConfig, tree_config: TreeConfig) -> PostFlopGame {
    let action_tree = ActionTree::new(tree_config).unwrap();
    PostFlopGame::with_config(card_config, action_tree).unwrap()
}

/// The game of the `flop_uncompressed` and `flop_compressed` golden scenarios. Both the turn and
/// the river are still to come, so the solve walks the whole tree of chance deals.
pub fn flop_game() -> PostFlopGame {
    game(
        card_config("Td9d6h", None, None),
        tree_config(BoardState::Flop),
    )
}

/// The game of the `turn_start` golden scenario, which is also the game of `examples/basic.rs`.
pub fn turn_game() -> PostFlopGame {
    game(
        card_config("Td9d6h", Some("Qc"), None),
        tree_config(BoardState::Turn),
    )
}

/// The game of the `river_start` golden scenario. The board is complete, so the solve exercises the
/// terminal evaluation and the river kernels alone.
pub fn river_game() -> PostFlopGame {
    game(
        card_config("Td9d6h", Some("Qc"), Some("2s")),
        tree_config(BoardState::River),
    )
}

/// The game of the `bunching` golden scenario, with the bunching effect already applied. Both
/// players hold every hand, and two folded ranges are removed from the deck.
pub fn bunching_game() -> PostFlopGame {
    let flop = flop_from_str("Td9d6h").unwrap();

    let card_config = CardConfig {
        flop,
        range: [Range::ones(); 2],
        turn: card_from_str("Qc").unwrap(),
        ..Default::default()
    };

    let tree_config = TreeConfig {
        initial_state: BoardState::Turn,
        starting_pot: 60,
        effective_stack: 970,
        river_bet_sizes: [("50%", "").try_into().unwrap(), Default::default()],
        ..Default::default()
    };

    let action_tree = ActionTree::new(tree_config).unwrap();
    let mut game = PostFlopGame::with_config(card_config, action_tree).unwrap();

    let co_range = "33:0.59,22:0.635,A8o:0.265,A7o-A6o,A5o:0.445,A4o-A2o,K2s,K9o:0.905,K8o-K2o,Q4s-Q2s,Q9o-Q2o,J6s-J2s,J9o:0.88,J8o-J2o,T7s:0.405,T6s-T2s,T9o:0.96,T8o-T2o,96s-92s,92o+,86s:0.57,85s-82s,82o+,76s:0.37,75s-72s,72o+,65s:0.475,64s-62s,62o+,54s:0.68,53s-52s,52o+,42+,32";
    let sb_range = "66:0.46,55:0.821,44:0.92,33:0.93,22:0.925,A6s:0.73,A3s:0.47,A2s,ATo:0.105,A9o-A2o,K8s:0.795,K7s,K6s:0.85,K5s:0.965,K4s-K2s,KJo:0.085,KTo:0.645,K9o-K2o,Q8s-Q2s,QJo:0.765,QTo-Q2o,J8s-J2s,J2o+,T8s:0.69,T7s-T2s,T2o+,98s:0.905,97s-92s,92o+,87s:0.78,86s-82s,82o+,76s:0.77,75s-72s,72o+,65s:0.845,64s-62s,62o+,54s:0.735,53s-52s,52o+,42+,32";

    let mut bunching_data = BunchingData::new(
        &[co_range.parse().unwrap(), sb_range.parse().unwrap()],
        flop,
    )
    .unwrap();

    process_bunching_data(&mut bunching_data);
    game.set_bunching_effect(&bunching_data).unwrap();

    game
}

/// Runs the bunching preprocessing on a single thread.
///
/// `BunchingData` accumulates its tables into atomic 64-bit floats, so the sums it produces depend
/// on the order in which the parallel workers reach them. Restricting the work to one thread pins
/// that order and makes the benchmarked game the same game that the golden test solves. The solve
/// itself runs on whatever pool the benchmark is started with.
fn process_bunching_data(bunching_data: &mut BunchingData) {
    #[cfg(feature = "rayon")]
    rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap()
        .install(|| bunching_data.process(false));

    #[cfg(not(feature = "rayon"))]
    bunching_data.process(false);
}
