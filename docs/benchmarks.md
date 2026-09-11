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
players, and of the equities of both players. Recording stops after 400 nodes, and the count sits
in the header of the file so that a change in the shape of the tree also shows up.

Run the suite with:

```
cargo test --release --test golden
```

It takes around twenty seconds on a ten-core machine.

When the numbers change on purpose, regenerate the files and read the diff before committing them:

```
UPDATE_GOLDEN=1 cargo test --release --test golden
```

`UPDATE_GOLDEN` makes every test overwrite its file and pass, so it must never be set in CI.

A failing test prints the first ten differing lines with both the expected and the actual value. If
the change was not intended, the report says which node moved and which of the five quantities
moved with it. A different `nodes` count means the tree itself has a different shape. The
comparison ignores lines that start with `#`, so the platform recorded in the header of each file
never causes a failure.

### Determinism

The numeric core of the solver uses only addition, subtraction, multiplication, division, `sqrt`
and `powi`. The first five are correctly rounded under IEEE 754, and the one call to `powi` has a
small constant exponent and compiles to a fixed chain of multiplications. The parallel parts of the
solve write to disjoint slices and reduce sequentially. Digests are therefore expected to hold
across platforms and across thread counts.

`thread_count_does_not_change_digest` checks the thread half of that claim directly. It solves the
same game on a pool of one thread and on a pool of four and asserts that the two digests are equal.

If a run on another platform disagrees with the checked-in files, that is a result worth chasing
rather than a reason to introduce a tolerance.

One part of the crate does depend on thread scheduling. The preprocessing in `BunchingData`
accumulates its tables into atomic 64-bit floats, so parallel workers reach those counters in
whatever order they finish and the sums differ in their last bits. The `bunching` scenario
therefore runs that preprocessing on a pool of one thread. Everything after it, including the solve
and the walk, is order independent.
