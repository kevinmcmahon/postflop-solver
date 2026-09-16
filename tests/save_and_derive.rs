//! Native saved games preserve the public data used to export alternate lines.
#![cfg(feature = "bincode")]

use postflop_solver::*;

fn solved_game(compressed: bool) -> PostFlopGame {
    let cards = CardConfig {
        range: ["QQ+,AKs".parse().unwrap(), "JJ-99,AQs".parse().unwrap()],
        flop: flop_from_str("8h7h2h").unwrap(),
        ..Default::default()
    };
    let sizes = BetSizeOptions::try_from(("50%", "")).unwrap();
    let tree = TreeConfig {
        initial_state: BoardState::Flop,
        starting_pot: 100,
        effective_stack: 100,
        flop_bet_sizes: [sizes.clone(), sizes.clone()],
        turn_bet_sizes: [sizes.clone(), sizes.clone()],
        river_bet_sizes: [sizes.clone(), sizes],
        ..Default::default()
    };
    let mut game = PostFlopGame::with_config(cards, ActionTree::new(tree).unwrap()).unwrap();
    game.allocate_memory(compressed);
    let hands = game.private_cards(0).len();
    let mut lock = vec![0.0; hands * game.available_actions().len()];
    lock[0] = 0.25;
    lock[hands] = 0.75;
    game.lock_current_strategy(&lock);
    solve(&mut game, 8, 0.0, false);
    game
}

fn save(game: &PostFlopGame, compression: Option<i32>) -> Vec<u8> {
    let mut bytes = Vec::new();
    save_data_into_std_write(game, "originating solve metadata", &mut bytes, compression).unwrap();
    bytes
}

fn load(bytes: &[u8], limit: Option<u64>) -> Result<(PostFlopGame, String), String> {
    load_data_from_std_read(&mut &bytes[..], limit)
}

fn bits(values: &[f32]) -> Vec<u32> {
    values.iter().map(|value| value.to_bits()).collect()
}

fn compare_node(live: &mut PostFlopGame, loaded: &mut PostFlopGame) {
    assert_eq!(live.history(), loaded.history());
    assert_eq!(live.available_actions(), loaded.available_actions());
    assert_eq!(live.current_player(), loaded.current_player());
    live.cache_normalized_weights();
    loaded.cache_normalized_weights();
    assert_eq!(bits(&live.strategy()), bits(&loaded.strategy()));
    for player in 0..2 {
        assert_eq!(live.private_cards(player), loaded.private_cards(player));
        for (expected, actual) in [
            (
                live.weights(player).to_vec(),
                loaded.weights(player).to_vec(),
            ),
            (
                live.normalized_weights(player).to_vec(),
                loaded.normalized_weights(player).to_vec(),
            ),
            (live.equity(player), loaded.equity(player)),
            (live.expected_values(player), loaded.expected_values(player)),
        ] {
            assert_eq!(
                bits(&expected),
                bits(&actual),
                "history {:?}",
                live.history()
            );
        }
    }
}

fn play_action(game: &mut PostFlopGame, action: Action) {
    let index = game
        .available_actions()
        .iter()
        .position(|a| *a == action)
        .unwrap();
    game.play(index);
}

fn compare_line(
    live: &mut PostFlopGame,
    loaded: &mut PostFlopGame,
    bet_flop: bool,
    turn: &str,
    river: &str,
) {
    live.back_to_root();
    loaded.back_to_root();
    for game in [&mut *live, &mut *loaded] {
        if bet_flop {
            play_action(game, Action::Bet(50));
            play_action(game, Action::Call);
        } else {
            play_action(game, Action::Check);
            play_action(game, Action::Check);
        }
        game.play(card_from_str(turn).unwrap() as usize);
    }
    compare_node(live, loaded);
    for game in [&mut *live, &mut *loaded] {
        play_action(game, Action::Check);
        play_action(game, Action::Check);
        game.play(card_from_str(river).unwrap() as usize);
    }
    compare_node(live, loaded);
    for game in [&mut *live, &mut *loaded] {
        play_action(game, Action::Check);
    }
    compare_node(live, loaded);
}

fn round_trip(compressed: bool, file_compression: Option<i32>) {
    let mut live = solved_game(compressed);
    live.set_target_storage_mode(BoardState::River).unwrap();
    let bytes = save(&live, file_compression);
    let memory = live.target_memory_usage();
    assert!((bytes.len() as u64) < memory);
    assert_eq!(
        load(&bytes, Some(memory - 1)).err().unwrap(),
        "Estimated memory usage is too large"
    );
    let (mut loaded, memo) = load(&bytes, Some(memory)).unwrap();
    assert_eq!(memo, "originating solve metadata");
    assert_eq!(loaded.storage_mode(), BoardState::River);
    assert_eq!(loaded.target_memory_usage(), memory);
    assert!(loaded.is_solved());
    compare_node(&mut live, &mut loaded);

    // On this monotone flop, the non-heart suits share chance representatives.
    play_action(&mut live, Action::Check);
    play_action(&mut live, Action::Check);
    let representative_cards = live.available_actions();
    assert!(representative_cards.len() < live.possible_cards().count_ones() as usize);
    assert!(["3c", "3d", "3s"].iter().any(|card| {
        !representative_cards.contains(&Action::Chance(card_from_str(card).unwrap()))
    }));
    for bet_flop in [false, true] {
        for (turn, river) in [("3c", "4d"), ("3d", "4s"), ("3s", "4c"), ("3h", "4h")] {
            compare_line(&mut live, &mut loaded, bet_flop, turn, river);
        }
    }
}

#[test]
fn full_depth_uncompressed_round_trip() {
    round_trip(false, None);
}

#[test]
fn full_depth_compressed_round_trip() {
    round_trip(true, None);
}

#[cfg(feature = "zstd")]
#[test]
fn full_depth_zstd_round_trip() {
    round_trip(true, Some(1));
}

#[test]
fn shallow_saves_retain_only_the_selected_streets() {
    let mut live = solved_game(true);
    for mode in [BoardState::Flop, BoardState::Turn] {
        live.back_to_root();
        live.set_target_storage_mode(mode).unwrap();
        let (mut loaded, _) = load(&save(&live, None), None).unwrap();
        assert_eq!(live.storage_mode(), BoardState::River);
        assert_eq!(loaded.storage_mode(), mode);
        assert!(loaded.set_target_storage_mode(BoardState::River).is_err());
        compare_node(&mut live, &mut loaded);
        if mode == BoardState::Turn {
            for game in [&mut live, &mut loaded] {
                play_action(game, Action::Check);
                play_action(game, Action::Check);
                game.play(card_from_str("3d").unwrap() as usize);
            }
            compare_node(&mut live, &mut loaded);
        }
    }
}
