# List of breaking changes

## 2026-09-16

- Document the existing full-depth save/load contract for deriving alternate lines. Full river
  saves omit value buffers; loading restores them and requires full-game memory, not file-size
  memory. Shallower saves discard later streets and retain their browsing values.
- Add exact native round-trip tests for compressed and uncompressed games, alternate actions,
  isomorphic runouts, shallow storage and the loader's estimated-memory check. No engine API,
  file format or solver behavior changes.

## 2026-09-15

- The crate is built with Rust edition 2024 and requires Rust 1.96.0 or later. `rust-toolchain.toml` pins that version and `Cargo.lock` is tracked, so every build uses the same compiler and the same dependency versions.
- `bincode` and `bincode_derive` are pinned to exactly `2.0.0-rc.3`. bincode 2.0.0 added a `Context` generic to the `Decode` trait, which the codec in this crate does not implement.
- `zstd` is updated to 0.14, `rayon` to 1.12 and `regex` to 1.13. The `once_cell` dependency is removed in favour of `std::sync::LazyLock`.
- `apply_swap` no longer forms two aliasing references to the same slice, an error Miri reported. An out-of-range swap index is a panic instead of silent memory corruption.
- `tests/golden.rs` pins the output of the solver on nine scenarios and `benches/solve.rs` measures the time a solve takes on five of them. See [docs/benchmarks.md](docs/benchmarks.md) for how to run both and what a refactor of the solver has to show.
- CI runs the gates on the pinned stable toolchain, checks the `wasm32-unknown-unknown` target and the build without default features, and runs the `custom-alloc` build and Miri on nightly. Dependabot proposes dependency updates weekly.

## 2023-10-01

- `BetSizeCandidates` and `DonkSizeCandidates` are renamed to `BetSizeOptions` and `DonkSizeOptions`, respectively.

## 2023-02-23

- `available_actions()` method of `PostFlopGame` now returns `Vec<Action>` instead of `&[Action]`.

## 2022-12-13

- revert: `compute_exploitability` function is back, and `compute_mes_ev_average` function is removed.

## 2022-12-11

- `TreeConfig`: new fields `rake_rate` and `rake_cap` are added.
- real numbers in `BetSize` enum  and `TreeConfig` struct are now represented as `f64` instead of `f32`.
- `compute_exploitability` function is renamed to `compute_mes_ev_average`.

## 2022-12-07

- `PostFlopGame`:
  - `play`: now terminal actions can be played.
  - `is_terminal_action` method is removed and `is_terminal_node` method is added.
  - `expected_values` and `expected_values_detail` methods now take a `player` argument.

## 2022-12-02

- `ActionTree`: `new` constructor now takes a `TreeConfig` argument.
- `ActionTree`: `with_config` and `update_config` methods are removed.

## 2022-11-30

- `TreeConfig`: `merging_threshold` field is added.
- `PostFlopGame`: `private_hand_cards` method is renamed to `private_cards`.

## 2022-11-29

- struct `GameConfig` is split into `CardConfig` and `TreeConfig`.
- new struct `ActionTree` is added: takes `TreeConfig` for instantiation.
- now `PostFlopGame` takes `CardConfig` and `ActionTree` for instantiation.
- `add_all_in_threshold` and `force_all_in_threshold` are renamed to `add_allin_threshold` and `force_allin_threshold`, respectively (`all_in` -> `allin`).
- `adjust_bet_size_before_all_in` (renamed from `adjust_last_two_bet_sizes`) is removed.

## 2022-11-27

- enum `BetSize` has new variants: `Additive(i32)`, `Geometric(i32, f32)`, and `AllIn`.
- `BetSize::LastBetRelative` is renamed to `BetSize::PrevBetRelative`.
- `BetSizeCandidates::try_from()` method is refactored. See the documentation for details. Now a pot-relative size must be specified with the '%' character, and the `try_from()` method rejects a single floating number.
- `adjust_last_two_bet_sizes` field of `GameConfig` struct is renamed to `adjust_bet_size_before_all_in`.

## 2022-11-14

- struct `GameConfig` has new fields: `turn_donk_sizes` and `river_donk_sizes`. Their types are `Option<DonkSizeCandidates>`. Specify these as `None` to maintain the previous behavior.
