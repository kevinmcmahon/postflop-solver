# AGENTS.md

Instructions for an agent working in this repository. Read this before planning or changing code.

## Purpose

`postflop-solver` is a Rust library that solves postflop Texas hold'em spots with the Discounted CFR
algorithm. It is the backend engine for two GUI applications, WASM Postflop and Desktop Postflop.
This repository is Kevin McMahon's fork of `b-inary/postflop-solver`, whose upstream development
stopped in October 2023. [README.md](README.md) describes what the library does.
[docs/benchmarks.md](docs/benchmarks.md) describes how it is measured.

The engine is performance critical and carries a lot of unsafe code. A flop solve runs for seconds
to minutes and touches gigabytes of node storage, so node values live behind raw pointers into a few
flat byte buffers, the borrow checker is bypassed by a mutex-shaped wrapper that performs no
locking, and the recursion fans out over rayon. Treat every part of that machinery as load bearing.

## Core rules

1. The golden suite is the behaviour oracle. `tests/golden.rs` solves eight fixed scenarios, walks
   each solved tree through the public interface, and compares a text digest byte for byte against a
   checked-in file in `tests/golden/`. A change to the internals is behaviour preserving when those
   digests are unchanged. There is no tolerance and there is no second oracle.
2. Never regenerate a golden file as part of a change that was not meant to move the numbers.
   `UPDATE_GOLDEN=1` overwrites every digest and passes every test, which destroys the oracle.
   Regenerate only when adding a scenario, and filter the run to that scenario's test name.
3. The numeric core is bit deterministic and has to stay that way. The same solve yields the same
   digest on a pool of one thread and on a pool of four, and with rayon compiled out of the crate
   altogether. `thread_count_does_not_change_digest` in `tests/golden.rs` checks the thread half of
   that claim. Do not add a reduction whose result depends on the order parallel workers finish.
4. `bincode` and `bincode_derive` are pinned to exactly `2.0.0-rc.3`. bincode 2.0.0 added a
   `Context` generic to the `Decode` trait, which `src/game/serialization.rs` does not implement.
   The two pins move together: the requirement rc.3 declares on `bincode_derive` is loose enough to
   resolve to the incompatible 2.0.1 release, which carries the same trait change.
5. Do not relax a gate so a change passes. `--deny warnings` applies to builds, clippy and rustdoc.
   Silencing a lint at the call site needs a reason written next to it.
6. `rust-toolchain.toml` pins the stable toolchain and `Cargo.lock` is tracked. The channel named in
   `.github/workflows/rust.yml` has to stay in step with the file, because the toolchain action
   takes the channel as an input and does not read it.

## Ask first

Propose the change and wait for an answer before touching any of these.

- `src/solver.rs`, the Discounted CFR recursion and the discount schedule.
- `src/game/node.rs` and the storage layout it reads: byte widths, scale factors, the raw pointers
  in `PostFlopNode`, and the flat buffers in `PostFlopGame` that they point into.
- `src/game/serialization.rs`, which rebases those pointers on load and defines the file format.
- Anything that changes a golden digest, including a change to a scenario's configuration.
- The `bincode` pin, the pinned toolchain version, or the edition.
- The behaviour of `MutexLike`, which supplies `Send` and `Sync` without locking.

## Verification matrix

Every command below runs from the repository root. The stable commands use the pinned toolchain from
`rust-toolchain.toml`. The nightly ones name `+nightly` explicitly, because that file overrides
rustup's default.

| Area changed | Run |
| --- | --- |
| Anything at all | Baseline gates |
| `src/`, `tests/`, `examples/` | Baseline gates, then the full stable gate |
| Solver internals or storage layout: `src/solver.rs`, `src/utility.rs`, `src/sliceop.rs`, `src/game/` | Full stable gate, the bench comparison, and Miri |
| Unsafe code, raw pointers, or `MaybeUninit` handling | Full stable gate and Miri |
| Feature-gated code or `Cargo.toml` features | Full stable gate, which covers the no-default-features build and the wasm32 target, plus the nightly gate |
| `src/alloc.rs` or anything under `#[cfg(feature = "custom-alloc")]` | Full stable gate and the nightly gate |
| `benches/` | `cargo bench --bench solve --no-run --features zstd`, then the bench comparison |
| `.github/workflows/` or `.github/dependabot.yml` | `actionlint` |
| Documentation only | `cargo doc` and the golden suite once, to show the oracle still holds |

### Baseline gates

```sh
cargo fmt --all --check
RUSTFLAGS="--deny warnings" cargo clippy --release --features zstd --all-targets -- -A clippy::needless_range_loop
RUSTDOCFLAGS="--deny warnings" cargo doc --release
```

`clippy::needless_range_loop` is allowed in every clippy invocation, in CI and here. It fires in
`src/bunching.rs`, `src/game/base.rs`, `tests/kuhn.rs` and `tests/leduc.rs`. Pass the same `-A` when
running clippy by hand, or the output will not match CI.

### Full stable gate

This is the list `.github/workflows/rust.yml` runs, in order. The test step takes about 20 seconds,
most of it the golden suite.

```sh
RUSTFLAGS="--deny warnings" cargo build --release --features zstd
RUSTFLAGS="--deny warnings" cargo build --release --no-default-features --features bincode
cargo check --target wasm32-unknown-unknown --no-default-features --features bincode
RUSTFLAGS="--deny warnings" cargo test --release --features zstd
RUSTFLAGS="--deny warnings" cargo clippy --release --features zstd --all-targets -- -A clippy::needless_range_loop
cargo fmt --all --check
RUSTDOCFLAGS="--deny warnings" cargo doc --release
RUSTFLAGS="--deny warnings" cargo bench --bench solve --no-run --features zstd
cargo run --release --example basic
cargo run --release --example file_io
cargo run --release --example node_locking
```

The golden suite on its own, which is the fastest useful signal after a change to the solver:

```sh
cargo test --release --test golden
```

### Nightly gate

`custom-alloc` needs nightly for the allocator API. The allocator hands out memory from a per-thread
stack and panics unless each thread frees its allocations in the reverse order it made them, so the
tests run one at a time.

```sh
cargo +nightly build --release --features custom-alloc
cargo +nightly test --release --features custom-alloc -- --test-threads 1
cargo +nightly clippy --release --features custom-alloc -- -A clippy::needless_range_loop
```

### Miri

Miri checks the aliasing and initialization rules the raw-pointer storage depends on. It interprets
MIR and is far too slow for `PostFlopGame`, so it covers the small Kuhn and Leduc adapters in
`tests/`. Those exercise the same `Game` and `GameNode` traits and the same `MaybeUninit` slice
handling. Both tests solve 20 iterations under `cfg!(miri)` and skip the convergence assertion:
Miri is there for aliasing, not for convergence.

```sh
cargo +nightly miri test --test kuhn --test leduc
```

The run takes several minutes.

### Bench comparison

Measure the code before the change, then the code after it, on one machine in one sitting.
Baselines live under `target/criterion`, which is build output and is not committed.

```sh
RAYON_NUM_THREADS=1 cargo bench --bench solve -- --save-baseline before
RAYON_NUM_THREADS=1 cargo bench --bench solve -- --baseline before
```

A single-thread run of all five scenarios takes about ten minutes. Record the multi-thread numbers
as a secondary check by repeating both commands with `RAYON_NUM_THREADS` unset.
[docs/benchmarks.md](docs/benchmarks.md) holds the baseline table, the machine hygiene the numbers
assume, the rule for accepting a refactor, and a recipe for reading the generated assembly when the
timings move less than the code generation did.

## Repository map

### `src/`

| File | Contents |
| --- | --- |
| `lib.rs` | Crate documentation, the module list, and the public re-exports. |
| `interface.rs` | The `Game` and `GameNode` traits. The seam between the solver and any game that can be solved. |
| `solver.rs` | The Discounted CFR recursion, `solve` and `solve_step`, and the rayon fan-out. |
| `utility.rs` | Exploitability and expected-value computation, `finalize`, the compressed-slice encode and decode helpers, `apply_swap`, and the SIMD max helpers. |
| `sliceop.rs` | Inlined elementwise kernels over `f32` slices that the solver calls in its hot loops. |
| `action_tree.rs` | `ActionTree`, `Action` and `TreeConfig`. Builds the betting tree from a configuration and applies added and removed lines. |
| `bet_size.rs` | `BetSizeOptions` and `DonkSizeOptions`, including the string format they parse. |
| `range.rs` | `Range`, the parsing and formatting of hand-range strings. Uses `regex` behind a `LazyLock`. |
| `card.rs` | The `Card` alias, board and card parsing, `CardConfig`, and the isomorphism swap lists. |
| `hand.rs` | The `Hand` accumulator used to look up a made hand's strength. |
| `hand_table.rs` | A static table of 4824 hand-strength values. Generated data, not hand edited. |
| `bunching.rs` | `BunchingData` and the three-phase preprocessing of the bunching effect. |
| `atomic_float.rs` | `AtomicF32` and `AtomicF64` over the matching atomic integers, used by the bunching preprocessing. |
| `mutex_like.rs` | `MutexLike` and `MutexGuardLike`, which supply `Send` and `Sync` and interior mutability without locking anything. |
| `file.rs` | Save and load, including the file header, the zstd layer, and the bincode codec entry points. Gated on the `bincode` feature. |
| `alloc.rs` | The per-thread stack allocator behind `custom-alloc`. Nightly only. |
| `game/mod.rs` | The `PostFlopGame` and `PostFlopNode` struct definitions. The field layout of the node storage lives here. |
| `game/base.rs` | Construction, configuration, memory sizing and allocation, and the `Game` implementation for `PostFlopGame`. |
| `game/node.rs` | The `GameNode` implementation for `PostFlopNode`. Turns the raw pointers and element counts into slices. |
| `game/evaluation.rs` | Terminal and showdown evaluation, with and without the bunching effect. |
| `game/interpreter.rs` | The public API for walking a solved tree: history, available actions, strategy, equity, expected values, and node locking. |
| `game/serialization.rs` | The bincode `Encode` and `Decode` implementations, including rebasing the node pointers on load. Gated on the `bincode` feature. |
| `game/tests.rs` | Unit tests for the game module. |

### `tests/`

| File | Contents |
| --- | --- |
| `golden.rs` | The behaviour oracle. Eight scenarios, the digest format, and the thread-independence test. |
| `golden/*.txt` | The checked-in digests, one per scenario. |
| `kuhn.rs` | A Kuhn poker adapter implementing `Game` and `GameNode`. Small enough for Miri. |
| `leduc.rs` | A Leduc hold'em adapter, with isomorphism and compression. Small enough for Miri. |

### `benches/`

| File | Contents |
| --- | --- |
| `solve.rs` | Criterion benchmarks over five of the golden scenarios, with each iteration count and its measured time beside it. |
| `scenarios/mod.rs` | The game configurations the benchmarks build. A subdirectory, because cargo turns every `benches/*.rs` file into a bench target. |

### `examples/`

| File | Contents |
| --- | --- |
| `basic.rs` | Builds a flop game, solves it, and reads the results. The commented walkthrough of the API. |
| `file_io.rs` | Saves a solved game and loads it back. |
| `node_locking.rs` | Locks a strategy at a node, fully and partially, before solving. |

### `docs/`

| File | Contents |
| --- | --- |
| `benchmarks.md` | The golden suite, the criterion benchmarks, the determinism argument, the baseline table, the codegen check, and the acceptance rule for solver refactors. |
| `specs/` | Design documents, one per dated topic. |
| `plans/` | Implementation plans, one per dated topic. |

## Architecture constraints

The `Game` and `GameNode` traits in `src/interface.rs` are the seam the solver works against. Three
adapters implement them: `PostFlopGame` in `src/game/`, and the Kuhn and Leduc games in `tests/`.
The Kuhn and Leduc adapters are small enough to run under an interpreter, which is what the nightly
Miri job uses them for. A change to either trait has to be carried through all three adapters.

Node storage is one arena and a handful of flat byte buffers. `PostFlopGame` owns
`node_arena: Vec<MutexLike<PostFlopNode>>` plus `storage1`, `storage2`, `storage_ip` and
`storage_chance` as `Vec<u8>`. Each `PostFlopNode` holds three raw pointers into those buffers, the
element counts of the slices they address, and a scale factor for each of the three.
`PostFlopNode::children` finds its children by an offset from its own address inside the arena, so
the arena has to stay one contiguous allocation and the nodes have to stay in the order allocation
put them. The pointers do not survive a save and a load, and `src/game/serialization.rs` rebases
them.

Knowledge of that layout is spread. The element width, whether a buffer holds `f32` or `i16`, and
where a given node's slice starts are each computed in `src/game/mod.rs`, `src/game/base.rs`,
`src/game/node.rs`, `src/game/serialization.rs`, `src/game/interpreter.rs` and `src/utility.rs`. A
layout change has to be made in all six or it corrupts memory rather than failing to compile. Giving
the layout one owner is open work, described under follow-ups.

`MutexLike` performs no locking. Safety comes from the solver writing to disjoint slices, not from
mutual exclusion. Any change that lets two workers reach the same slice is a data race even though
it compiles.

Compression is a second representation of every stored value, as `i16` with an `f32` scale per node
rather than `f32`. Both paths have to be maintained together, and the golden suite covers both:
`flop_compressed` and `monotone_flop` store compressed values, the other six store floats.

`BunchingData::process` accumulates into atomic floats from parallel workers, so its output depends
on the order the workers finish. The `bunching` golden scenario runs that preprocessing on a pool of
one thread to pin it. Everything after the preprocessing, including the solve and the walk, is order
independent. Until the accumulation is fixed, bunching preprocessing has to run single threaded
anywhere a reproducible result is required.

`f64::powf` in `src/action_tree.rs` computes geometric bet sizes and is a libm call rather than a
correctly rounded IEEE operation. It is the one place where a platform difference could reach the
digests, and it would show as a different `nodes` count rather than as drift in the values.

## Documentation map

Update documentation in the same commit as the change it describes.

| Change | Document |
| --- | --- |
| Public API, features, installation, how to run an example | `README.md` |
| A change consumers have to react to | `CHANGES.md`, as a dated section at the top |
| Performance measurement, golden scenarios, baselines, the acceptance rule | `docs/benchmarks.md` |
| Commands, gates, repository layout, architecture constraints, follow-ups | This file |
| A design worth recording before it is built | `docs/specs/<date>-<topic>.md` |
| The steps for building it | `docs/plans/<date>-<topic>.md` |

Doc comments in `src/lib.rs` repeat the implementation-details and crate-features sections of
`README.md`. Changing one means changing the other.

## Known follow-ups

None of these are scheduled. Each is a real defect or a real cost, recorded so it is not rediscovered.

- Migrate the codec to bincode 2 or 3. The crate is held at `=2.0.0-rc.3` for both `bincode` and
  `bincode_derive`, so it cannot take security or bug fixes from the stable line.
- Fix the bunching nondeterminism. `BunchingData::process` accumulates into atomic `f64` counters
  from parallel workers, so the sums differ in their last bits between runs. Callers who need a
  reproducible result have to pin the preprocessing to one thread, which is what `tests/golden.rs`
  does.
- Assert the swap-list precondition. `src/card.rs:392-398` builds a swap pair from a reverse-table
  lookup that yields `usize::MAX` when the suit-swapped hand is absent from the range, which
  narrows to the index 65535. `apply_swap` panics on such an index rather than corrupting memory,
  and nothing checks the precondition at the point the pair is built.
- Give the node storage layout one owner. Byte widths, scale arithmetic and pointer offsets are
  computed in the six files listed under architecture constraints, so a layout change has to be
  made correctly in all six. Consolidating that arithmetic behind one type is the change that would
  most reduce the risk of working on the solver.
- Bring the wasm32 max helpers in line. The `simd128` variants of the two max helpers in
  `src/utility.rs` still use `chunks_exact`, while the portable variants use `as_chunks`.
- Decide what `custom-alloc` is for. It requires nightly, duplicates four function bodies behind
  `cfg`, and exists for a multi-threaded WASM environment whose default allocator is slow. Either
  the feature earns its keep or it goes.
- Expect the nightly job to break. It runs with `--deny warnings` on a moving toolchain, so each
  round of lints lands as a red build with no change to this repository.

## Worktrees

Branch work happens in a git worktree under `.worktrees/<branch>` in the repository root.
`.worktrees/` is listed in `.gitignore`. Remove the worktree and delete the branch after the work
lands on `main`.

The git stash stack is shared across every worktree. Use a temporary commit to set work aside rather
than a bare `git stash`.

The developer machine carries a global gitignore that excludes `AGENTS.md` and `CLAUDE.md`, so both
were committed with `git add -f`. The rule no longer applies to them as tracked files, but any file
added later under either name needs the same forced add.

## Issue tracker

This repository has no issue tracker. Report follow-up work in the final message of a task and add
it to the follow-ups section above when it is worth keeping.
