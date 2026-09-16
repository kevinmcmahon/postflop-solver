# postflop-solver

> [!NOTE]
> This repository is Kevin McMahon's fork of [b-inary/postflop-solver].
> The original author suspended development of the upstream project in October 2023 to build a poker
> solver as a business. [This issue][this issue] carries the announcement.
> This fork is maintained for the downstream apps and for further work on the solver.

[b-inary/postflop-solver]: https://github.com/b-inary/postflop-solver
[this issue]: https://github.com/b-inary/postflop-solver/issues/46

---

An open-source postflop solver library written in Rust

Documentation: https://b-inary.github.io/postflop_solver/postflop_solver/

**Related repositories**
- Web app (WASM Postflop): https://github.com/b-inary/wasm-postflop
- Desktop app (Desktop Postflop): https://github.com/b-inary/desktop-postflop

**Note:**
The primary purpose of this library is to serve as a backend engine for the GUI applications ([WASM Postflop] and [Desktop Postflop]).
The direct use of this library by the users/developers is not a critical purpose by design.
Therefore, breaking changes are often made without version changes.
See [CHANGES.md](CHANGES.md) for details about breaking changes.

[WASM Postflop]: https://github.com/b-inary/wasm-postflop
[Desktop Postflop]: https://github.com/b-inary/desktop-postflop

## Usage

- `Cargo.toml`

```toml
[dependencies]
postflop-solver = { git = "https://github.com/kevinmcmahon/postflop-solver" }
```

- Examples

You can find examples in the [examples](examples) directory.

If you have cloned this repository, you can run the example with the following command:

```sh
$ cargo run --release --example basic
```

## Saving a solved tree and deriving another line

With the default `bincode` feature, the existing native API saves a solved tree and loads it for
traversal. Select full river depth to keep alternate actions and runouts on every street:

```rust,ignore
game.set_target_storage_mode(BoardState::River)?;
save_data_to_file(&game, &memo, "solved.bin", None)?;

let (mut loaded, memo): (PostFlopGame, String) =
    load_data_from_file("solved.bin", Some(max_memory_bytes))?;
loaded.back_to_root();
// At player nodes, validate an action against available_actions(), then play its index.
// At chance nodes, validate the card against possible_cards(), then play its card ID.
loaded.cache_normalized_weights();
let strategy = loaded.strategy();
let reach = loaded.weights(loaded.current_player());
let normalized_reach = loaded.normalized_weights(loaded.current_player());
```

This derives a line within the saved equilibrium. It does not solve a new subgame or change the
ranges, board, betting tree or solve settings. Keep those settings and the originating solve result
in the memo or a separate provenance record. The native game alone does not record iteration counts,
discount parameters or the originating exploitability report. See [examples/file_io.rs](examples/file_io.rs)
for the file API. `save_data_to_file` overwrites an existing destination; callers that need atomic
publication or protection against overwriting must provide it themselves.

Full `BoardState::River` serialization stores strategy storage, card and tree configuration, tree
edits, node topology, scales, locks and other metadata. It omits counterfactual value buffers.
Loading allocates those buffers again and calls `finalize` to rebuild values for a solved game.
There are no CFR iterations, but loading has a computation cost and needs full-game memory.
`target_memory_usage()` reports an estimate for that loaded game, not the file's byte length.
The loader checks the header's estimate against `max_memory_bytes`; that is not a hard cap on
process memory or transient allocations.

`BoardState::Flop` and `BoardState::Turn` save only the selected streets and retain the values
needed to browse them. They discard later node storage in the file. Selecting a shallow target
does not truncate the live game, but loading that file cannot recover later streets or upgrade it
to full river depth. Use full depth for line derivation. Optional zstd file compression is separate
from the engine's 16-bit value compression.

`tests/save_and_derive.rs` checks exact strategy, reach, normalized reach, equity and value bits
on alternate actions and runouts, including isomorphic suits, with both engine storage widths.
These tests cover ordinary postflop games. They do not establish a persistence contract for the
bunching effect or compatibility across engine revisions; consumers should validate their saved
record's revision and configuration before traversal.

## Implementation details

- **Algorithm**: The solver uses the state-of-the-art [Discounted CFR] algorithm.
  Currently, the value of γ is set to 3.0 instead of the 2.0 recommended in the original paper.
  Also, the solver resets the cumulative strategy when the number of iterations is a power of 4.
  `SolveParams` selects the discount exponents and the restart schedule that `solve_with_params` and
  `solve_step_with_params` use. `SolveParams::current()` is this default, and `SolveParams::paper()`
  chooses the paper's schedule instead.
- **Performance**: The solver engine is highly optimized for performance with maintainable code.
  The engine supports multithreading by default, and it takes full advantage of unsafe Rust in hot spots.
  The developer reviews the assembly output from the compiler and ensures that SIMD instructions are used as much as possible.
  Combined with the algorithm described above, the performance surpasses paid solvers such as PioSOLVER and GTO+.
- **Isomorphism**: The solver does not perform any abstraction.
  However, isomorphic chances (turn and river deals) are combined into one.
  For example, if the flop is monotone, the three non-dealt suits are isomorphic, allowing us to skip the calculation for two of the three suits.
- **Precision**: 32-bit floating-point numbers are used in most places.
  When calculating summations, temporary values use 64-bit floating-point numbers.
  There is also a compression option where each game node stores the values by 16-bit integers with a single 32-bit floating-point scaling factor.
- **Bunching effect**: At the time of writing, this is the only implementation that can handle the bunching effect.
  It supports up to four folded players (6-max game).
  The implementation correctly counts the number of card combinations and does not rely on heuristics such as manipulating the probability distribution of the deck.
  Note, however, that enabling the bunching effect increases the time complexity of the evaluation at the terminal nodes and slows down the computation significantly.

[Discounted CFR]: https://arxiv.org/abs/1809.04040

## Crate features

- `bincode`: Uses [bincode] crate to serialize and deserialize the `PostFlopGame` struct.
  This feature is required to save and load the game tree.
  The dependency is pinned to exactly `2.0.0-rc.3`, because bincode 2.0.0 added a `Context` generic
  to the `Decode` trait and the codec in this crate has not been migrated to that API.
  `bincode_derive` carries the same exact pin: the dependency rc.3 declares on it is loose enough to
  resolve to the incompatible 2.0.1 release, which carries the same trait change.
  Enabled by default.
- `custom-alloc`: Uses custom memory allocator in solving process (only available in nightly Rust).
  It significantly reduces the number of calls of the default allocator, so it is recommended to use this feature when the default allocator is not so efficient.
  Note that this feature assumes that, at most, only one instance of `PostFlopGame` is available when solving in a program.
  Disabled by default.
- `rayon`: Uses [rayon] crate for parallelization.
  Enabled by default.
- `zstd`: Uses [zstd] crate to compress and decompress the game tree.
  This feature is required to save and load the game tree with compression.
  Disabled by default.

[bincode]: https://github.com/bincode-org/bincode
[rayon]: https://github.com/rayon-rs/rayon
[zstd]: https://github.com/gyscos/zstd-rs

## Development

`tests/golden.rs` pins the output of the solver so that a change to the internals can be shown to
leave the numbers alone, and `benches/solve.rs` measures how long a solve takes on the same games.
[docs/benchmarks.md](docs/benchmarks.md) explains how to run both, how to record a baseline, and
what a refactor of the solver has to show before it is accepted.

[AGENTS.md](AGENTS.md) holds the rest: the commands each area of the repository has to pass, a map
of the source tree, the constraints the architecture places on a change, and the list of known
follow-up work.

## License

Copyright (C) 2022 Wataru Inariba

This program is free software: you can redistribute it and/or modify it under the terms of the GNU Affero General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.

This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU Affero General Public License for more details.

You should have received a copy of the GNU Affero General Public License along with this program.  If not, see <https://www.gnu.org/licenses/>.
