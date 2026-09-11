//! Golden-output regression tests for the solver.
//!
//! Each test builds a game from a fixed configuration, solves a fixed number of iterations, walks
//! the solved tree through the public interface, and writes a deterministic text digest. The
//! digest is compared byte for byte with a checked-in file under `tests/golden/`.
//!
//! The tests exist so that a change to the internals of the solver can be shown to preserve
//! behaviour: the digests are sensitive to the strategy, the expected values, the equities, the
//! exploitability, and the shape of the tree.
//!
//! When a change to the numbers is intentional, regenerate the files with
//! `UPDATE_GOLDEN=1 cargo test --release --test golden` and review the resulting diff. Never set
//! `UPDATE_GOLDEN` in CI: it turns every one of these tests into a no-op.

use postflop_solver::*;
use std::fmt::Write as _;
use std::path::PathBuf;

/// Upper bound on the number of player nodes recorded per scenario. The walk stops descending once
/// the bound is reached, which keeps the digest files small without making them less deterministic.
const MAX_RECORDED_NODES: usize = 400;

/// Maximum number of cards explored at each chance node, taken in ascending card order.
const MAX_CHANCE_CHILDREN: usize = 2;

const OOP_RANGE: &str = "66+,A8s+,A5s-A4s,AJo+,K9s+,KQo,QTs+,JTs,96s+,85s+,75s+,65s,54s";
const IP_RANGE: &str = "QQ-22,AQs-A2s,ATo+,K5s+,KJo+,Q8s+,J8s+,T7s+,96s+,86s+,75s+,64s+,53s+";

// ---------------------------------------------------------------------------
// Scenarios
// ---------------------------------------------------------------------------

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
fn tree_config(initial_state: BoardState, rake_rate: f64, rake_cap: f64) -> TreeConfig {
    let bet_sizes = BetSizeOptions::try_from(("60%, e, a", "2.5x")).unwrap();

    TreeConfig {
        initial_state,
        starting_pot: 200,
        effective_stack: 900,
        rake_rate,
        rake_cap,
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

/// The game of the `set_bunching_effect` test in `src/game/tests.rs`, with the bunching effect
/// already applied. Both players hold every hand, and two folded ranges are removed from the deck.
fn bunching_game() -> PostFlopGame {
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
/// that order and makes the scenario reproducible. Everything after this point, including the whole
/// solve, is order independent.
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

/// Locks part of OOP's strategy at the root, which must happen after allocating memory and before
/// solving. Every third hand is pinned to 25% of the first action and 75% of the second one; the
/// other hands are left for the solver to adjust.
fn lock_oop_root(game: &mut PostFlopGame) {
    let num_actions = game.available_actions().len();
    let num_hands = game.private_cards(0).len();

    let mut strategy = vec![0.0; num_actions * num_hands];
    for hand in (0..num_hands).step_by(3) {
        strategy[hand] = 0.25;
        strategy[hand + num_hands] = 0.75;
    }

    game.lock_current_strategy(&strategy);
}

#[test]
fn flop_uncompressed() {
    let game = game(
        card_config("Td9d6h", None, None),
        tree_config(BoardState::Flop, 0.0, 0.0),
    );
    check_golden("flop_uncompressed", game, false, 30, None);
}

#[test]
fn flop_compressed() {
    let game = game(
        card_config("Td9d6h", None, None),
        tree_config(BoardState::Flop, 0.0, 0.0),
    );
    check_golden("flop_compressed", game, true, 30, None);
}

#[test]
fn turn_start() {
    let game = game(
        card_config("Td9d6h", Some("Qc"), None),
        tree_config(BoardState::Turn, 0.0, 0.0),
    );
    check_golden("turn_start", game, false, 50, None);
}

#[test]
fn river_start() {
    let game = game(
        card_config("Td9d6h", Some("Qc"), Some("2s")),
        tree_config(BoardState::River, 0.0, 0.0),
    );
    check_golden("river_start", game, false, 100, None);
}

#[test]
fn monotone_flop() {
    let game = game(
        card_config("8h7h2h", None, None),
        tree_config(BoardState::Flop, 0.0, 0.0),
    );
    check_golden("monotone_flop", game, true, 30, None);
}

#[test]
fn raked_river() {
    let game = game(
        card_config("Td9d6h", Some("Qc"), Some("2s")),
        tree_config(BoardState::River, 0.05, 30.0),
    );
    check_golden("raked_river", game, false, 100, None);
}

#[test]
fn node_locking() {
    let game = game(
        card_config("Td9d6h", None, None),
        tree_config(BoardState::Flop, 0.0, 0.0),
    );
    check_golden("node_locking", game, false, 30, Some(lock_oop_root));
}

#[test]
fn bunching() {
    check_golden("bunching", bunching_game(), false, 20, None);
}

/// Iterations used by the thread-independence test. It solves the `flop_compressed` game twice,
/// once on a single thread, so it costs far more wall time per iteration than the scenarios above.
/// Ten iterations keep the whole suite inside its time budget and still put every parallel fan-out
/// in the tree to work.
#[cfg(feature = "rayon")]
const THREAD_INDEPENDENCE_ITERATIONS: u32 = 10;

/// The solver fans out over rayon, so the digest must not depend on how many threads take part.
#[cfg(feature = "rayon")]
#[test]
fn thread_count_does_not_change_digest() {
    let digest = |threads: usize| {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        pool.install(|| {
            let mut game = game(
                card_config("Td9d6h", None, None),
                tree_config(BoardState::Flop, 0.0, 0.0),
            );
            solve_and_digest(
                "flop_compressed",
                &mut game,
                true,
                THREAD_INDEPENDENCE_ITERATIONS,
                None,
            )
        })
    };

    let one_thread = digest(1);
    let four_threads = digest(4);

    // `compare` reports the first differing lines instead of printing two digests in full.
    if let Some(report) = compare(&one_thread, &four_threads) {
        panic!("the digest of `flop_compressed` depends on the thread count\n{report}");
    }
}

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// Solves the game, with the node values stored as 16-bit integers when `compressed` is set, and
/// compares its digest with the checked-in golden file.
fn check_golden(
    name: &str,
    mut game: PostFlopGame,
    compressed: bool,
    iterations: u32,
    lock: Option<fn(&mut PostFlopGame)>,
) {
    let digest = solve_and_digest(name, &mut game, compressed, iterations, lock);
    let path = golden_path(name);

    if std::env::var("UPDATE_GOLDEN").as_deref() == Ok("1") {
        let dir = path.parent().unwrap();
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(&path, &digest).unwrap();
        return;
    }

    let expected = std::fs::read_to_string(&path).unwrap_or_else(|err| {
        panic!(
            "cannot read the golden file for scenario `{name}` at {}: {err}\n\
             run `UPDATE_GOLDEN=1 cargo test --release --test golden` once to create it",
            path.display()
        )
    });

    if let Some(report) = compare(&expected, &digest) {
        panic!(
            "golden digest mismatch for scenario `{name}` ({})\n{report}\n\
                if the change is intended, regenerate with \
                `UPDATE_GOLDEN=1 cargo test --release --test golden` and review the diff",
            path.display()
        );
    }
}

fn golden_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join(format!("{name}.txt"))
}

/// Allocates memory, applies the optional node lock, solves, and returns the digest text.
fn solve_and_digest(
    name: &str,
    game: &mut PostFlopGame,
    compressed: bool,
    iterations: u32,
    lock: Option<fn(&mut PostFlopGame)>,
) -> String {
    game.allocate_memory(compressed);

    if let Some(lock) = lock {
        lock(game);
    }

    let exploitability = solve(game, iterations, 0.0, false);
    game.back_to_root();

    let mut lines = Vec::new();
    walk(game, &mut lines);

    let mut digest = String::new();
    writeln!(digest, "# golden output for scenario {name}").unwrap();
    writeln!(
        digest,
        "# recorded on {} {} (informational; the comparison ignores lines starting with `#`)",
        std::env::consts::ARCH,
        std::env::consts::OS
    )
    .unwrap();
    writeln!(digest, "nodes {}", lines.len()).unwrap();
    writeln!(
        digest,
        "exploitability {:08x} {:.9e}",
        exploitability.to_bits(),
        exploitability
    )
    .unwrap();
    for line in lines {
        writeln!(digest, "{line}").unwrap();
    }

    digest
}

/// Walks the solved tree depth first and appends one line per player node.
///
/// The walk visits a player node before its children, descends into every action in order, and
/// descends into the first [`MAX_CHANCE_CHILDREN`] cards of every chance node. There is no undo
/// operation in the public interface, so the position is restored by replaying the saved history
/// from the root after each child.
fn walk(game: &mut PostFlopGame, lines: &mut Vec<String>) {
    if lines.len() >= MAX_RECORDED_NODES || game.is_terminal_node() {
        return;
    }

    let saved = game.history().to_vec();

    if game.is_chance_node() {
        let cards = game.possible_cards();
        let children = (0..52).filter(|card| cards & (1 << card) != 0);
        for card in children.take(MAX_CHANCE_CHILDREN) {
            game.play(card);
            walk(game, lines);
            game.apply_history(&saved);
            if lines.len() >= MAX_RECORDED_NODES {
                return;
            }
        }
        return;
    }

    game.cache_normalized_weights();
    lines.push(player_node_line(game, &saved));
    if lines.len() >= MAX_RECORDED_NODES {
        return;
    }

    for action in 0..game.available_actions().len() {
        game.play(action);
        walk(game, lines);
        game.apply_history(&saved);
        if lines.len() >= MAX_RECORDED_NODES {
            return;
        }
    }
}

/// Formats the digest line of the current player node.
fn player_node_line(game: &PostFlopGame, history: &[usize]) -> String {
    let position = if history.is_empty() {
        "root".to_string()
    } else {
        history
            .iter()
            .map(|action| action.to_string())
            .collect::<Vec<_>>()
            .join(" ")
    };

    let mut line = position;
    for (label, values) in [
        ("strategy", game.strategy()),
        ("ev0", game.expected_values(0)),
        ("ev1", game.expected_values(1)),
        ("eq0", game.equity(0)),
        ("eq1", game.equity(1)),
    ] {
        write!(
            line,
            " | {label} {:016x} {:.9e}",
            fnv1a64(&values),
            mean(&values)
        )
        .unwrap();
    }

    line
}

/// FNV-1a over the little-endian bit patterns of the values.
fn fnv1a64(values: &[f32]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for value in values {
        for byte in value.to_bits().to_le_bytes() {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    hash
}

/// Arithmetic mean of the values, accumulated in 64-bit precision. An empty slice has mean zero.
fn mean(values: &[f32]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let sum: f64 = values.iter().map(|&value| value as f64).sum();
    sum / values.len() as f64
}

/// Compares two digests, ignoring comment lines. Returns a report of the first differing lines, or
/// `None` when the digests agree.
fn compare(expected: &str, actual: &str) -> Option<String> {
    let strip = |text: &str| -> Vec<String> {
        text.lines()
            .filter(|line| !line.starts_with('#'))
            .map(|line| line.to_string())
            .collect()
    };

    let expected = strip(expected);
    let actual = strip(actual);

    if expected == actual {
        return None;
    }

    let mut report = format!(
        "expected {} lines, got {} lines; the counts and the line numbers below \
         leave out the comment lines\n",
        expected.len(),
        actual.len()
    );

    let missing = "<missing>".to_string();
    let differing = (0..expected.len().max(actual.len()))
        .map(|i| {
            (
                i,
                expected.get(i).unwrap_or(&missing),
                actual.get(i).unwrap_or(&missing),
            )
        })
        .filter(|(_, expected, actual)| expected != actual);

    for (i, expected, actual) in differing.take(10) {
        let _ = writeln!(
            report,
            "line {}:\n  expected: {expected}\n  actual:   {actual}",
            i + 1
        );
    }

    Some(report)
}
