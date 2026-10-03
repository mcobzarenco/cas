# Developing cas

```sh
cargo run --features dev            # dynamic linking: much faster incremental builds
cargo test --release --workspace
cargo clippy --release --workspace --all-targets
```

## Test rig

The app can drive itself from a tiny script (`--script`, or `--script-file`; screenshots go to
`--shots`, `shots/` unless given), which is how the UI gets exercised and screenshotted during
development. The scripts in `rig/` cover the panel, the rule menu and editor, the vacuum handling,
the spaceship catcher and the analysis of a pattern; each exits with status 1 at the first
expectation that fails:

```sh
cargo run --release -- --script "wait 20; shot start; click PlayPause; wait 60; shot running; quit"
for script in rig/*.cas; do cargo run --release -- --script-file $script || break; done
```

| command | effect |
|---------|--------|
| `wait N` | idle for N frames |
| `shot NAME` | save `shots/NAME.png` (see `--shots`) and wait until it is written |
| `film NAME FRAMES [N]` | FRAMES screenshots `NAME-000.png`, `NAME-001.png`, …, each followed by a step of N generations (1 unless given; negative goes backwards) |
| `click NAME [DX DY]` | pointer move / press / release on the UI node named NAME, optionally offset from its centre in logical pixels. Nodes: `PlayPause`, `StepBack`, `StepForward`, `Reverse`, `HideVacuum`, `ShowGrid`, `ShowBlocks`, `FitView`, `Soup`, `Blob`, `Cloud`, `Clear`, the sliders `Speed`, `Stride`, `Density`, `Grid`; the rule menu `RuleMenu` and its items `RuleItem:<preset id>`, `RuleItemCustom`; `EditRule`; in the editor `Out0` … `Out15` (the outcomes), `RuleIdentity`, `RuleInverse`, `RuleRandom`, `RuleRepresentative`, `RuleString`, `RuleEspca` (Morita's number under it), `RuleCopy`, `RulePaste`, `EditorClose`; the size menu `GridSize` and its items `GridSize:<side>`; `OpenBorder`, `Catching`, `Spaceships`; in the spaceship list `CatcherCatching`, `Kind0` … (the kinds, in the order they were first caught), `AnalyseKind0` … (their ◎ buttons), `CatcherForget`, `CatcherNote` (the line next to it), `CatcherClose`; `Analyse` and `SelectHint` (the line next to it); in the analysis panel `SubjectRule` (the rule's name next to the title), `AnalysisView`, `SmallCaption` (the line under it), `AnalysisPause`, `AnalysisRestart`, `SmallStatus` (the small world's generation, and what left it), `StudyWhat`, `StudyPeriod`, `StudySpeed`, `StudyCells`, `StudyChanges`, `StudySize`, `StudySymmetry`, `StudyPieces`, `StudyText`, `AnalysisPlace`, `AnalysisCopy`, `AnalysisNote`, `AnalysisClose` |
| `move NAME [DX DY]` | just move the pointer there |
| `drag NAME DX DY [left\|right\|middle]` | press at the node's centre, move by (DX, DY), release: drags sliders, paints, pans |
| `hold NAME FRAMES` | keep the left button down on the node for FRAMES frames |
| `scroll NAME LINES` | turn the wheel over the node |
| `key KEY` | press and release a key or chord: `Space`, `ArrowLeft`, `r`, `[`, `=`, `Ctrl+a`, ... |
| `press KEY`, `release KEY` | hold a key across other commands: `press Shift; drag Grid 60 0; release Shift`; or a mouse button (`left`, `right`, `middle`) where the pointer last was: `move Grid 0 0; press left; move Grid 60 60; shot band; release left` |
| `type TEXT` | type text into whatever has keyboard focus |
| `paint X Y [on\|off]` | set a cell |
| `place RLE X Y` | put a run-length encoded pattern with its corner at (X, Y) |
| `fit` | fit the view |
| `play`, `pause`, `step N`, `rule RULE` (preset or table), `speed N`, `stride N`, `vacuum on\|off`, `reverse on\|off`, `soup [DENSITY]`, `blob [DENSITY]`, `cloud [DENSITY]`, `clear` | direct state changes |
| `expect_gen N`, `expect_cell X Y on\|off`, `expect_population N`, `expect_rule RULE`, `expect_speed N`, `expect_stride N`, `expect_playing on\|off`, `expect_size W H`, `expect_checked NAME on\|off`, `expect_text NAME TEXT` (a button reads as its caption), `expect_caught SHIPS KINDS`, `expect_clipboard TEXT` | fail the run unless the state is as stated (cells and population are the pattern's, without the vacuum) |
| `quit` | exit |

Commands are separated by `;` or newlines, `#` starts a comment. Pointer and keyboard actions are
injected as the messages `bevy_winit` would emit, so they go through picking, focus and the widgets
like real input (the window's own cursor position is left alone; winit would warp the real cursor).
Real input is discarded while a script runs and the window lets the pointer through, so a run does
not depend on what else happens at the machine. Naming a UI node that does not exist fails the run.

Example: `shot a; step 500; step -500; expect_gen 0; shot b` produces two byte-identical PNGs.

## The pictures of the README

The figures in `docs/` are drawn by `cargo run --release -p cas-core --example figures`, with
the code that steps the grid, so they show what the rules do. The screenshots and the animation
are taken by the test rig: `docs/tour.cas` and `docs/hero.cas` say how in their first lines (it
takes `ffmpeg`, and `oxipng` to shrink the PNGs).

## Layout

A cargo workspace of three crates. `cas-core` is the automata without the app and knows nothing
of Bevy; the app at the root and the search program are built on it.

| file | what |
|------|------|
| `crates/cas-core/src/rules.rs` | rules as permutation tables: presets, text form, Morita's numbers, swaps, inverse, the vacuum's cycle and the rule relative to it, analysis (population, weighted counts of cells, symmetry, time reversal), the rule that stands for all that only look different; tests check each preset against its definition and the numbers against the book's statements |
| `…/universe.rs` | the grid and its stepping kernel, the vacuum kept apart from the cells, resizing, what the edge does (open border, catching). Tests hold the kernel against the plain definition of a step and replay the spaceships published with Single Rotation (which pins rotation sense, bit layout and phase to the reference simulator) |
| `…/pattern.rs` | finite patterns on an unbounded plane: what becomes of a pattern left alone, its period, displacement and canonical form, taking apart patterns that only travel together, the full study of one (symmetry, heat, growth law, what it came apart into), run-length encoding. Tests use the periods and displacements js-revca's tests give, hold the analysis against the grid, and replay figures of Morita's book (which pins down how his automata lie on the block grid) |
| `…/census.rs` | counting by kind the spaceships a universe caught |
| `…/search.rs` | the trials a rule is put through, the cheap ones first, and its report |
| `…/families.rs` | the families of rules a search goes through: by symmetry, by what is conserved, at random |
| `crates/cas-core/examples/figures.rs` | draws the figures of the README |
| `crates/cas-search` | the command-line search: the table, taking up an interrupted search, a closer look at the best of a table |
| `src/sim.rs` | the universe in the app: transport and pacing, the settings, the system sets that order a frame; the pacing is tested in a headless app |
| `src/catcher.rs` | the spaceship list: identifies what was caught at the edge within a time budget per frame, for each rule, shows the panel, and hands a kind to the stamp when its row is clicked, or to the analysis by the row's small button |
| `src/analysis.rs` | the analysis panel: a pattern chosen with a band on the grid or sent from the list is studied by `cas_core::pattern::Analyser` (`Study`: fate, motion, symmetry, heat, growth, pieces) and shown living in a small universe of its own, drawn with the grid's material; a pattern that never repeats gets an open border there, and what leaves is counted by a `Census` |
| `src/actions.rs` | everything the user can ask for as one `Action` enum with a single handler; the key table, which also labels the controls; hold-to-repeat stepping; who gets the keyboard |
| `src/view.rs` | the grid node: view state (zoom / pan / fit), the UI material and its parameters (shared with the analysis panel's small view), painting and navigation via picking events, the band drawn around a pattern to analyse, and the stamp: a pattern picked up from the spaceship list or the analysis panel, shown as a ghost and put down with a click |
| `src/grid.wgsl` | the fragment shader: view transform, cell colours, the vacuum under the cells, grid and block overlays, the band, the ghost of the stamp |
| `src/ui.rs` | the control panel as cards, one per aspect, and the aspects' colours (Bevy UI + `bevy_feathers` dark theme, `bsn!` scenes); sliders and checkboxes on the headless widgets, widget↔state sync |
| `src/editor.rs` | the rule editor panel: the sixteen cases, swap editing, the properties with their diagrams, the rule string and clipboard |
| `src/rig.rs`, `rig/*.cas` | the script-driven test rig and the scripts that exercise the app |
| `docs/` | the pictures of the README, and the rig scripts that take its screenshots |

Stepping backwards from generation *g* applies the inverse table with the partition the forward
step *g−1 → g* used (even generations: blocks aligned with the origin; odd: shifted by (1, 1)).

Stepping: cells are one byte each. The kernel walks the grid a pair of rows at a time, loads eight
cells of each row at once and looks two blocks up per table access; a 256×256 generation takes
about 5 µs on one core, a 4096×4096 one about 0.3 ms on six. Small grids are stepped on the calling
thread, large ones are shared out with rayon (`--threads`; the step is bound by memory, so a
handful of threads is as fast as all of them). `cargo test --release -p cas-core -- --ignored
--nocapture stopwatch` times it.

Drawing: the cell array is uploaded as-is into an `R8Uint` texture (one byte per cell, a plain
`memcpy` when the universe changes) and a UI material's fragment shader does everything else, so
zooming, panning and toggling overlays cost nothing on the CPU. Zoomed out, the shader looks at
every cell under a pixel (up to 8×8) and lets any live cell show, so sparse patterns stay visible.

## Known gaps

The limitations users meet are in the README. Besides those:

* Catching with a closed border on a grid full of soup is slow, six to nine times a plain step at
  256×256: everything on the edge is debris, and it is looked at again after every generation.
* The paused app still redraws every frame; Bevy's reactive update mode would let it idle.
* Painting and wheel zoom assume a `UiScale` of 1.
* A scripted window still takes the keyboard focus when it opens. Its input is discarded, but
  keystrokes meant for the window behind it are lost while it is in front.

## Environment notes

* Bevy 0.19.1 comes from crates.io; a checkout of that tag next to this repository (`../bevy`) is
  handy as the API reference, for its `examples/`. Bevy's features are enumerated explicitly in
  `Cargo.toml` (no 3D, audio or gamepads). `cas-core` has an optional `bevy` feature, which the
  app turns on: it only makes the universe and the random generator Bevy resources.
  Native Wayland is the crate's default `wayland` feature and needs `libwayland-dev` at build time;
  `cargo build --no-default-features` falls back to winit's X11 backend (XWayland).
* Vsync: `--vsync auto` (the default) waits for vsync only for native Wayland windows, where it
  works fine on both the NVIDIA and the Intel adapter (~60 fps; ~150 fps with `--vsync off`).
  Under XWayland `PresentMode::AutoVsync` stalls the frame loop to about 1 fps, hence the default.
