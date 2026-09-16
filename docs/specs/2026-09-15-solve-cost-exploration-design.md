# Solve cost exploration: design

Date: 2026-09-15. Repositories: postflop-solver (this fork) and hand-ranger.

## Goal

Lower the cost per accepted hand for the 200bb round of hand-ranger without moving graded references beyond the agreed tolerance. A hand is two solves of the same tree (600 and 1,000 iterations) plus a stability check; it is accepted when every graded step of the line agrees within 0.03 total variation and final exploitability is at or below 0.5 percent of the pot. The number to minimize is dollars per accepted hand on the K♣7♠2♦ 200bb rich tree (36.9 GB compressed), with peak memory alongside, because memory decides the instance and how many solves share it, and per-iteration time decides the bill.

Two capabilities matter more than speed and land first: solve parameters (discount exponents and the averaging restart schedule) and a way to derive other lines on a solved flop without re-solving. Each amended line last round cost a fresh two-hour solve.

## Facts the design rests on

- Iterations are full-tree traversals and memory-bandwidth bound: 4 to 6 seconds per iteration at 4 cores on 4 to 7 GB trees. Every byte removed from node storage is paid back on every iteration.
- The fork's golden suite (`tests/golden.rs`) solves the `flop_compressed` scenario at one thread and at four threads and compares the two digests byte for byte, and all eight scenarios reproduce with rayon disabled; that is the evidence that the engine is bit-deterministic across thread counts. The hand-ranger record (ADR-0011) treats the multithreaded build as not bit-repeatable, and the 0.03 tolerance remains the contract between the repositories. The criterion benches in `benches/solve.rs` guard throughput.
- The hand-ranger adapter (`adapters/postflop-solver`) pins upstream b-inary at revision 9d1509f. No engine change reaches a dataset solve until the adapter is repointed at this fork, which is a hand-ranger decision recorded in an ADR superseding ADR-0008.
- The reference hand hr-0002 is the J♠9♠6♦ two-tone tree at 100bb: 7.8 GB uncompressed, 4.0 GB compressed. Its pinned-build exports (`solver-export.json` at 600 iterations, `stability-export.json` at 1,000) are committed under `dataset/hands/hr-0002`, and `hand-ranger-gen stability --reuse-export <export> --hand dataset/hands/hr-0002` grades any export in protocol format against `hand.json`'s references. The recorded agreement between the two pinned-build solves (`stability.json`) is four graded steps at total variation 0.0072 (cp-2), 0.0114 (cp-6), 0.0106 (cp-8), and 0.0233 (cp-11), so the worst step sits at 0.023 against the 0.03 limit; final exploitability was 0.57 chips at 600 iterations and 0.53 at 1,000 against the 2.75 chip ceiling.
- hand-ranger's `docs/research/tree-probe-200bb/full-solve-k72-200bb-tree01` holds a full solve of the 10 GB `k72-200bb-tree01` tree on tree/0.1: 10.9 seconds per iteration on 4 cores, 182 minutes for 1,000 iterations, exploitability 0.72 chips at 600 iterations and 0.76 at 1,000, and all six Villain steps stable with a worst total variation of 0.022.
- The engine's native save (`save_data_to_file`) writes the strategy storage a browser needs and nothing else; loading a saved game and playing a line reproduces the path export without solving.
- Averaging today: discount exponents alpha 1.5, beta 0, gamma 3, and the cumulative strategy is zeroed at iterations 4, 16, 64, 256, 1,024. The paper's schedule is gamma 2 with no restart. Fixed counts of 600 and 1,000 were chosen to sit inside one restart window; under target-based stopping only four of ten hands passed the 0.03 check.

## Phase 1: gate, parameters, derivation

### 1a. Regression gate (first deliverable)

Purpose: prove a fork build reproduces hr-0002 within tolerance before any parameter or export work, so every later change is measured against a known-good fork build.

- `adapters/postflop-solver/gate.sh` in hand-ranger builds the adapter against a solver given as a local path or git revision, using a cargo `[patch]` override so the committed pin is untouched. The export's solver block does not come from Cargo metadata: the adapter embeds the constants `SOLVER_REPO` and `SOLVER_REV` from `src/main.rs`, which a `[patch]` override does not change. The adapter therefore reads `HR_SOLVER_REPOSITORY` and `HR_SOLVER_REVISION` at build time (`option_env!`, falling back to the constants), the gate passes the fork's URL and the built revision in, and the gate reads the solver block of each export and fails if the revision is not the one it built, including when it names the pinned upstream revision. It solves `dataset/hands/hr-0002/solver-request.json` as is (version 3: 600 iterations, target 0, check every 100) and the same request with `max_iterations` 1,000, writes both exports, runs the stability tool once per export, and prints the per-step total-variation table with a single pass or fail line.
- Pass rule, all three for both exports: every graded step within 0.03 of `hand.json`'s references; final exploitability at or below 2.75 chips (0.5 percent of 550); and the stability tool's `derivation_checks_failed` list empty (the solver cross-check of reach weights, policy sums, and plausibility), so a build cannot pass on total variation alone.
- The gate runs locally on the developer machine, where the adapter is built. Both solves take about 95 minutes at 4 cores. The script also accepts any other request for quick checks, without the pass rule.
- The pinned build's exports are not regenerated; the committed files are the baseline.
- Once the fork passes on its unchanged engine, the superseding ADR repoints the adapter, and it states that the 200bb round is solved entirely on the fork build that passed the gate.

### 1b. Solve parameters

- postflop-solver gains `SolveParams { alpha: f32, beta: f32, gamma: f32, restart: RestartSchedule }` where `RestartSchedule` is `PowersOfFour`, `None`, or `At(Vec<u32>)`. Named presets: `SolveParams::current()` (1.5, 0, 3, `PowersOfFour`) and `SolveParams::paper()` (1.5, 0, 2, `None`).
- `solve_with_params` and `solve_step_with_params` take the struct; the existing `solve` and `solve_step` call them with `current()`, so every existing golden stays byte-identical. The discount computation in `src/solver.rs` reads the struct instead of constants.
- Tests: a golden scenario on the paper preset; Kuhn and Leduc convergence under both presets; the existing goldens unchanged.
- Protocol 0.2 adds `solve.discount { alpha, beta, gamma, restart }` with `current` as the default when absent. The adapter still reads 0.1 requests, and regenerating a committed hand from its saved export stays byte for byte. The export echoes the block, and hand-ranger's generator carries it into `hand.json`'s configuration so the parameters a hand was solved under are part of its record. On the hand-ranger side solve settings live in the tree file's solve block, and tree/0.1 is frozen, so `solve.discount` goes into the new tree version the 200bb round needs anyway (tree/0.3, or a fresh name, with the richer sizes), and the generator passes it through to the protocol 0.2 request and into `hand.json`'s configuration.
- Two results to report separately, after the gate and the parameters land. First, two paper-preset solves of hr-0002's request, one stopped at the 0.5 percent target and one at half of it, compared with each other on every graded step; this is the ADR-0013 method and the actual test of whether fixed counts can go. Second, the paper-preset references compared with the committed fixed-count references, reported as information only: the paper schedule may settle on a different member of the equilibrium set, and disagreement there is not a failure.

### 1c. Line derivation from a saved game, optional and off by default

- Not a JSON dump. Strategies alone on the rich tree are 18 GB at 16 bits; JSON would triple that. The engine's native save already holds what derivation needs.
- Protocol 0.2 adds `export.save_game` (path; absent means off). When set, the adapter saves the solved game next to the export after writing it.
- The adapter gains a `derive` mode: given a saved game and a request whose `path` names any line, it loads the game and writes an export in the same node format as a fresh solve, with no solve.
- A derived export must satisfy the generator the same way a fresh one does. It echoes the derived request (same ranges, board, and tree, with the new path), because the generator refuses an export whose echo differs from the manifest's request. Its solve result carries the originating solve's `max_iterations`, `iterations_run`, and `exploitability`, because the generator checks those counts against the requested ones. It adds a `derived_from` block with the saved file's hash, the solve parameters, and the fork revision. hand-ranger keeps the solver block from the export, and a derived hand must be reproducible from that record.
- Ordering: the adapter writes the export first, then saves the game. The generator's `solver_export_sha256` covers the export file alone, so the hash never depends on whether a save happened or what it contains.
- Loading a saved game needs memory equal to the file, so rich-tree derivations run on the same instance class that solved them.
- Both solves of a hand (600 and 1,000) need saved games if a derived line is to pass the stability check. At up to 18 GB each they live in object storage, never in git.
- Engine work: a test that a saved-then-loaded game reproduces a path export bit for bit, and a check of what `save_data_to_file` writes at each storage mode so the file holds strategies only.

## Phase 2: cost model (prepared now, cloud runs gated on the AWS account and the Terraform review)

- A runner in hand-ranger drives the adapter on the probe requests under `docs/research/tree-probe-200bb` and records, per run: threads, seconds per iteration, peak resident memory, the request's memory figure, and the fork revision. Mac runs use `j96-200-tree01` (5.9 GB) and `k72-200bb-tree01` (10 GB) at 1, 2, 4, and 8 threads.
- The model: dollars per accepted hand equals 1,600 iterations times seconds per iteration times instance price per hour, divided by 3,600 and by the number of solves that fit in the instance's memory at once.
- The model is seeded from the full solve recorded in the design's facts: 10.9 seconds per iteration at 4 cores gives the 1,600-iteration figure directly, before any cloud run.
- Because iteration time is bandwidth bound, the runner also measures a plain streaming-bandwidth figure on the machine, so an instance's iteration time can be predicted from its bandwidth before renting it.
- Cloud validation: one Graviton instance and one x86 instance, one solve each of `k72-200bb-tree01` under the same request, on demand, terminated on completion. The result decides the default instance type in the Terraform being written now (currently r7a.4xlarge). The runbook is written in phase 2 and waits for the account.

## Phase 3: memory reductions, each gated by 1a and priced by phase 2

In payoff order, each on its own branch with its evidence and a before-and-after cost line:

1. Solve-only mode that skips the IP counterfactual value buffer. First measure the buffer's share of the rich tree's storage, and confirm `compute_exploitability` does not read it. The export uses no expected values.
2. Dead-hand elimination at turn and river nodes: hands blocked by the dealt cards are allocated today, up to 15 percent of the nodes that dominate storage. This rewrites the storage layout and is done as part of the node storage refactor from the architecture review, not separately.
3. Split precision (regrets and cumulative strategy at different widths). Last, because it can move references; the gate re-runs on hr-0002.

Dataset consistency: any change that moves references means a set is solved on one adapter pin per version of the set. The export records adapter and engine revision per hand, so mixed pins are visible.

Out of scope: river-tree parallelism, `custom-alloc`.

## Where things live

- postflop-solver: `SolveParams`, save-and-derive engine test, memory reductions, goldens and benches (a scaled-down scenario from `j96-200-tree01` joins both).
- hand-ranger: protocol 0.2, adapter (gate script, `derive` mode, discount pass-through), generator change for the configuration record, cost runner and runbook, superseding ADR for ADR-0008.
- The superseding ADR for ADR-0008 is a proposal until Kevin accepts it, per hand-ranger's documentation rule; the repoint happens after acceptance.
- Order across repositories, fixed: gate script, gate run on the unchanged fork, ADR proposal and acceptance, repoint, then engine change, adapter change, gate run, per section.


## 2026-09-16: accepted 1c storage and protocol correction

The original text above records the design as proposed. The accepted 1c plan corrects these
assumptions after inspecting the native codec:

- Full `BoardState::River` files contain strategy storage plus configuration, tree edits, topology,
  scales, locks and metadata. Counterfactual value buffers are omitted. On load the codec allocates
  them and calls `finalize` for a solved game. Loading performs no CFR iterations, but requires
  full-game memory and computation. File size is not a memory budget.
- Shallow targets retain value buffers needed for browsing and discard deeper node storage. Only
  full river saves qualify for alternate-line derivation through all streets. The existing
  `set_target_storage_mode`, save/load and traversal APIs suffice; no format change is required.
- hand-ranger save/derive controls belong in protocol 0.3, because 0.2 already contains solve
  parameters and ADR-0021 requires a bump for a new engine knob. Old export reuse remains supported.
- The adapter memo records the originating request, solver identity, complete solve result and
  original export hash. Derivation validates the saved configuration and revision, preserves the
  historical solve result and records the native file hash and reader identity. Saving follows
  export output, so opting in does not change the fresh export hash.
- Both 600- and 1,000-iteration games are required for derived generation and stability checks.
  They remain outside git. Local round-trip tests are not the hand-ranger acceptance gate: Kevin
  runs `adapters/postflop-solver/gate.sh` against the committed fork checkout on his Mac, and its
  actual record belongs under `docs/evidence/solver-gate/<fork-hash>/` before repinning the adapter.

Kevin amended the gate rule during 1c implementation: a test/docs-only fork commit may carry
forward a prior passing gate after verifying that production code and build inputs are unchanged.
Record both revisions and the comparison alongside dedicated save/derive integration checks.
Solver behavior, storage, serialization, dependency and build changes still require the full Mac
gate. This exception supersedes the unconditional run requirement above for the 1c test/docs unit;
it does not claim another two-solve gate was executed.
