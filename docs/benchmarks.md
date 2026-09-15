# Benchmarks and regression tests

## Golden output tests

`tests/golden.rs` pins the observable output of the solver. Each test builds a game from a fixed
configuration, solves a fixed number of iterations, walks the solved tree, and writes a text
digest. The digest is compared byte for byte with a checked-in file in `tests/golden/`. The tests
exist so that a change to the internals, such as the layout of the node storage, can be shown to
leave the numbers alone.

Eight scenarios cover the cases that take different paths through the solver. Four of them start on
the flop, two start on the turn and two start on the river. Two of them store the node values as
16-bit integers instead of 32-bit floats, and the rest use 32-bit floats. One flop scenario uses a
monotone board, which exercises the isomorphism of suits, and another locks part of the strategy of
the out-of-position player at the root before solving. One river scenario charges rake. The last
scenario applies the bunching effect.

The walk is a depth-first traversal through the public interface of `PostFlopGame`. It records one
line per player node before descending into that node's children, visits every action of a player
node in order, and takes the two lowest cards at each chance node. A line holds the action history
of the node followed by an FNV-1a hash and the mean of the strategy, of the expected values of both
players, and of the equities of both players. Above those lines the file records the exploitability
that the solve returned, both as a bit pattern and as a decimal, so the digest pins the convergence
of the solve as well as the values at the nodes. The traversal stops after 400 nodes, and the count
sits in the header of the file so that a change in the shape of the tree also shows up.

Ascending card order reaches the reference card of an isomorphism group before its duplicates, so
on a board with two unused suits the two lowest cards are both reference cards. Only `monotone_flop`
has three unused suits and therefore records a chance branch where the interpreter swaps suits on
the way in. The swap the solver itself performs, which folds isomorphic deals onto one subtree, runs
in every flop scenario.

Run the suite with:

```
cargo test --release --test golden
```

The scenarios are sized to keep the whole suite inside a budget of 30 seconds.

When the numbers change on purpose, regenerate the files and read the diff before committing them:

```
UPDATE_GOLDEN=1 cargo test --release --test golden
```

`UPDATE_GOLDEN` makes every test overwrite its file and pass, so it must never be set in CI.

A failing test walks the two digests in step and prints the expected line and the actual line for
each of the first ten positions where they differ. A different `nodes` count means the tree itself
has a different shape. The comparison ignores lines that start with `#`, so the platform recorded in
the header of each file never causes a failure.

### Determinism

The numeric core of the solver uses only addition, subtraction, multiplication, division, `sqrt`
and `powi`. The first five are correctly rounded under IEEE 754, and the one call to `powi` has a
small constant exponent and compiles to a fixed chain of multiplications. The parallel parts of the
solve write to disjoint slices and reduce sequentially. Digests are therefore expected to hold
across platforms and across thread counts.

`thread_count_does_not_change_digest` checks the thread half of that claim directly. It solves the
same game on a pool of one thread and on a pool of four and asserts that the two digests are equal.
All eight digests also come out unchanged from a build with
`--no-default-features --features bincode`, which leaves rayon out of the crate altogether.

One call sits outside that argument. The `powf` call in `src/action_tree.rs` computes geometric bet
sizes, and `f64::powf` is a libm call rather than a correctly rounded IEEE operation, so its last
bits may differ between platforms. The result is rounded to whole chips, so a difference has to
cross a rounding boundary to be visible at all, and when it does it changes a bet amount and
therefore the shape of the action tree. That surfaces as a different `nodes` count rather than as
drift in the values, and it is the first thing to check if a Linux run disagrees with the
checked-in files.

If a run on another platform disagrees with the checked-in files, that is a result worth chasing
rather than a reason to introduce a tolerance.

One part of the crate does depend on thread scheduling. The preprocessing in `BunchingData`
accumulates its tables into atomic 64-bit floats, so parallel workers reach those counters in
whatever order they finish and the sums differ in their last bits. The `bunching` scenario
therefore runs that preprocessing on a pool of one thread. Everything after it, including the solve
and the walk, is order independent.

## Throughput benchmarks

`benches/solve.rs` measures how long a solve takes on the games of the golden scenarios. Each
benchmark builds one game and then times a closure that allocates the node storage and runs a fixed
number of Discounted CFR iterations. The golden tests say whether a change moved the numbers. These
benchmarks say whether it moved the time.

Five scenarios run under the group name `solve`: `flop_uncompressed`, `flop_compressed`,
`turn_start`, `river_start` and `bunching`. Their configurations match the golden tests of the same
names, so both halves of the harness work on the same games. Integration tests and benchmarks
compile as separate crates and cannot share a module, so the configuration literals appear twice,
once in `tests/golden.rs` and once in `benches/scenarios/mod.rs`. That helper sits in a
subdirectory because cargo turns every `benches/*.rs` file into a bench target of its own, and a
second target here would run no benchmarks.

Run every scenario:

```
RAYON_NUM_THREADS=1 cargo bench --bench solve
```

Run one by name:

```
RAYON_NUM_THREADS=1 cargo bench --bench solve -- turn_start
```

Save the numbers of the current code under a name, then measure a later build against them:

```
RAYON_NUM_THREADS=1 cargo bench --bench solve -- --save-baseline before
RAYON_NUM_THREADS=1 cargo bench --bench solve -- --baseline before
```

The second command prints the change in each scenario together with the verdict criterion draws
from its own statistics, one of `Performance has improved`, `Performance has regressed`, `Change
within noise threshold` and `No change in performance detected`. Baselines live under
`target/criterion`, which is build output and stays out of the repository, so both halves of a
comparison have to be measured on one machine within one stretch of work.

### Why the single-thread number comes first

Treat `RAYON_NUM_THREADS=1` as the primary measurement. A machine that mixes performance cores with
efficiency cores lets the scheduler move a rayon worker from one kind to the other in the middle of
a solve. That variance is larger than the effects these benchmarks exist to catch, so a
multi-thread run can swallow a few percent lost to worse code generation. Record the multi-thread
number as well, because a change to the storage layout can alter how the cores share memory and the
single-thread run says nothing about that.

### Machine hygiene

Two runs are comparable only when the machine is in the same state for both. Keep the laptop plugged
in, quit anything else that uses the processor, and run the benchmark twice, keeping the second run.
A run that starts right after a long compile begins on a hot machine and reads slower than the same
code measured on a cool one.

### How the benchmarks are sized

Criterion collects ten samples per scenario, which is its minimum. Every scenario runs enough CFR
iterations that one solve takes between five and six seconds on a single thread, and the
measurement budget of 100 seconds puts two solves in each sample. Criterion prints a warning
whenever a sample comes out as a single solve, and the budget is chosen to keep that warning away.
The time criterion reports is the time of one solve, not of one sample. A whole single-thread run
takes about ten minutes.

The counts are tuned for the machine in the baseline table. Where one solve takes longer than ten
seconds, criterion falls back to one solve per sample, the warning returns, and the samples grow
rather than shrink. The answer is to lower the iteration constant for that scenario until a solve
lands back under ten seconds, and to record the new count next to the numbers it produced, since a
time measured at one count says nothing about a time measured at another.

`solve` recomputes the exploitability every ten iterations and once before the first one, which puts
a second kind of traversal inside the measurement. The ten-iteration flop scenarios spend two of
twelve traversals there, against roughly one in eleven for the turn and river scenarios. A
regression confined to `solve_recursive` therefore reads slightly smaller in the flop rows than in
the others.

The benchmarks ask for flat sampling, since a solve of several seconds is far too slow for the
linear sampling that criterion prefers by default. Flat sampling also decides which statistic
reaches the terminal: with no slope to fit, the middle number of the three criterion prints is the
mean of the samples, and the outer two are the bounds of its 95 percent confidence interval.

Each iteration count sits in a named constant in `benches/solve.rs` with its measured single-thread
time beside it. The counts differ by four orders of magnitude, because a river tree with the board
complete costs tens of microseconds per iteration while a flop tree with both the turn and the river
still to come costs more than half a second.

The storage allocation stays inside the timed closure. `allocate_memory` reallocates the storage and
returns the game to its unsolved state, which is what lets one game be solved once per iteration,
and it costs at most 0.13 percent of a solve on the largest scenario. Measuring it is cheaper than
the per-iteration setup that would be needed to take it out.

## Acceptance rule for solver refactors

A change to the internals of the solver, such as the layout of the node storage, is accepted when
three things hold.

1. Every golden test passes with a byte-identical digest. No tolerance, and no regenerated files.
2. No scenario is more than 2 percent slower in its single-thread time, counting only the scenarios
   where criterion calls the difference statistically significant. The number to read is the middle
   figure of the `change:` line, which is criterion's estimate of the relative difference in the
   mean; the `p = ... < ...` on the same line and the verdict sentence printed under it say whether
   the difference is significant. A scenario criterion reports as unchanged or as within the noise
   threshold carries no weight in either direction.
3. The multi-thread numbers are recorded next to the single-thread ones and read as a secondary
   check. A regression that appears only there points at sharing between cores rather than at code
   generation, and is worth understanding before the change goes in.

## Codegen check

The hot loops of this crate were tuned by reading the compiler output, so a change that leaves the
timings alone on this machine can still have moved the generated code in a way that costs elsewhere.
Reading the assembly answers that directly.

Install the tool once:

```
cargo install cargo-show-asm
```

`solve_recursive` is generic over `Game` and the library instantiates it nowhere, so `--lib` has no
monomorphized copy to print and reports that it cannot find the item. Dump it from a target that
solves a `PostFlopGame`:

```
cargo asm --release --example basic "postflop_solver::solver::solve_recursive"
```

That lists four matching items. Three are rayon shims that carry the name inside their generic
arguments. The one to read is the entry whose name is exactly
`postflop_solver::solver::solve_recursive`, with nothing wrapped around it, and it is far larger
than the other three. Pass its index back to print it, and repeat after the change:

```
cargo asm --release --example basic "postflop_solver::solver::solve_recursive" 3 > before.asm
cargo asm --release --example basic "postflop_solver::solver::solve_recursive" 3 > after.asm
diff before.asm after.asm
```

`--bench solve` reaches the same code and works as a source too.

Two things in the diff matter more than its size.

A new `bl` instruction inside a loop means a kernel or an accessor that used to inline no longer
does. The dump is not call free to begin with. It recurses into itself, calls `regret_matching` and
`regret_matching_compressed`, calls `slice_absolute_max` and `slice_nonnegative_max` from
`src/utility.rs`, and calls rayon's fan-out alongside the allocator shims and the panic paths. Every
kernel in `src/sliceop.rs` carries `#[inline]`, and no `sliceop` symbol appears in the dump at all,
so a call to one of those names is the clearest sign that a change broke an inline.

Fewer vector instructions mean the vectorizer gave up on a loop it used to handle. LLVM prints NEON
for this target in Apple's dot syntax, so the arithmetic to count is `fmul.4s`, `fadd.4s` and
`fsub.4s`, and the vector loads and stores are the `q` register forms of `ldp`, `stp`, `ldr` and
`str`. The output holds no `fmla` and no full width `ld1` or `st1`, so counting those finds nothing.

```
grep -c '\.4s' before.asm
grep -cE '\b(ldp|stp|ldr|str)[[:space:]]+q' before.asm
```

For the code the baseline table measures those counts are 194 and 160.

Native aarch64 output is a proxy for the target that matters most. The build that most users reach
is the wasm32 one that WASM Postflop ships, and its code generation differs. Benchmarking the wasm
build is follow-up work and is outside the scope of this document.

## Baseline

Recorded on 2026-09-11 on an Apple M5 with 10 cores, using rustc 1.96.0 (ac68faa20 2026-05-25) and
the default feature set. Each figure is the time of one solve: the storage allocation plus the
listed number of Discounted CFR iterations.

Baseline `main`, one thread, taken with `RAYON_NUM_THREADS=1`:

| Scenario | Iterations | Mean | 95 percent confidence interval |
| --- | --- | --- | --- |
| `flop_uncompressed` | 10 | 5.4508 s | 5.4211 s to 5.4815 s |
| `flop_compressed` | 10 | 5.7825 s | 5.7309 s to 5.8342 s |
| `turn_start` | 750 | 5.9852 s | 5.9355 s to 6.0345 s |
| `river_start` | 250000 | 5.6167 s | 5.5838 s to 5.6483 s |
| `bunching` | 50 | 5.9421 s | 5.8573 s to 6.0331 s |

Baseline `main-mt`, all ten cores, taken with `RAYON_NUM_THREADS` unset:

| Scenario | Iterations | Mean | 95 percent confidence interval |
| --- | --- | --- | --- |
| `flop_uncompressed` | 10 | 1.7953 s | 1.5051 s to 2.1002 s |
| `flop_compressed` | 10 | 2.1726 s | 2.1334 s to 2.2168 s |
| `turn_start` | 750 | 2.3344 s | 2.2839 s to 2.4126 s |
| `river_start` | 250000 | 5.7675 s | 5.7039 s to 5.8429 s |
| `bunching` | 50 | 2.1984 s | 2.1498 s to 2.2416 s |

Two features of the second table are worth carrying forward. The confidence interval of
`flop_uncompressed` spans a third of its own mean, which is the variance that makes the
single-thread run the primary measurement. And `river_start` gains nothing from ten cores: with the
board complete the tree is small enough that the cost of handing work to rayon cancels the work it
takes away.
