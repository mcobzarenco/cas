# cas

A sandbox for exploring **reversible block cellular automata**, built on [Bevy](https://bevy.org) 0.19.

Only *block* CAs on the **Margolus neighbourhood** are supported for now: the grid is cut into
2×2 blocks, every block is rewritten by a lookup table, and the partition shifts by one cell
diagonally between generations. Because each table is a permutation of the 16 block states, every
rule has an exact inverse and time can be run backwards.

A rule is its table: the outcome of each of the 16 block states, written as 16 comma-separated
numbers with the outcome of block 0 first. Cells of a block count 1, 2, 4, 8 for top-left, top-right,
bottom-left, bottom-right, so `0,2,8,3,1,5,6,7,4,9,10,11,12,13,14,15` is Single Rotation. This is the
notation of [dmishin's simulator](https://dmishin.github.io/js-revca) (and of MCell, with an `MS,D`
prefix and `;` separators), so rules can be exchanged with it.

Presets, all reversible:

| preset | what it does |
|--------|--------------|
| **Single rotation** | blocks with exactly one live cell rotate 90° clockwise ([dmishin](https://dmishin.blogspot.com/2013/11/the-single-rotation-rule-remarkably.html)) |
| **Critters** | 0, 1 or 4 live cells: invert the block; 2: keep it; 3: invert and rotate 180° ([Wikipedia](https://en.wikipedia.org/wiki/Critters_(cellular_automaton))) |
| **Billiard ball machine** | a lone cell crosses its block diagonally, a diagonal pair bounces to the other diagonal |
| **Bounce gas** | the billiard ball machine, with three-cell blocks turned by 180° as well |
| **HPP gas** | every block turns by 180°, except diagonal pairs, which scatter onto the other diagonal |
| **Tron** | empty and full blocks swap, nothing else changes |
| **Rotations** | one- and three-cell blocks rotate clockwise, two-cell blocks are complemented |
| **Double rotation** | one-cell blocks rotate clockwise, three-cell blocks counter-clockwise |
| **String thing** | only two-cell blocks change: they are complemented |
| **Swap on diagonal** | every block turns by 180° |

Rules from Morita's book *Reversible World of Cellular Automata* (2024), which calls them by
number (see below):

| preset | what it does |
|--------|--------------|
| **ESPCA-01c5ef** | one-cell blocks rotate counter-clockwise, three-cell blocks clockwise, diagonal pairs jump to the other diagonal. One of the four rules the book builds reversible Turing machines in; the signal is a spaceship of period 12 |
| **ESPCA-01caef** | the mirror image of Double rotation, and another of the four. Rich in spaceships |
| **ESPCA-02c5bf** | the billiard ball machine with three-cell blocks rotated counter-clockwise: the third; the billiard ball machine itself is the fourth, ESPCA-02c5df. Can simulate every reversible rule of the family |
| **ESPCA-016a7f** | one-cell blocks rotate counter-clockwise, adjacent pairs clockwise, three-cell blocks by 180°. Has a spaceship of period 3 |
| **ESPCA-0945df** | a lone cell gains a neighbour and two side by side lose it again: cells are not conserved. Oscillators of period 6, 22 and 60, a spaceship of period 3, and a gun: two full blocks side by side |
| **ESPCA-09457f** | the same with three-cell blocks turned by 180°. A single cell is a gun that sends out four spaceships every 8 steps, forwards and backwards in time |
| **ESPCA-098aef** | spaceships of period 10 and 17 |
| **ESPCA-0925bf** | a single cell grows into an almost perfect disk; nobody knows why |
| **ESPCA-0dca8f** | a single cell grows into shapes that look like fractals |

Anything else is a **custom** rule, made in the rule editor or given as a table.

### Morita's numbers

The rules that look the same after a quarter turn have a second name. Morita studies *elementary
square partitioned cellular automata* (ESPCA): square cells with four parts, each holding a
particle or not, where a cell's next state is a function of the parts of its four neighbours that
face it. Put a site on every edge between two cells, where a particle crosses: a cell takes in the
four sites around it and puts four out, which is a block being rewritten, and the cells that do so
on even steps and those in between on odd steps are the two partitions. An ESPCA is therefore two
block automata that never meet, each drawn turned by 45°: the book's north is up and to the right
here. Its speeds are the same numbers as the ones shown here, since it counts distance along the
axes of its grid, which are the diagonals of this one.

He numbers the rules with six hexadecimal digits, the outcomes of a cell with no particle coming
in, one, two at a right angle, two head-on, three and four. `ESPCA-01c5ef` (or `espca-01c5ef`) is
accepted wherever a rule is, and the rule editor shows the number of every rule that has one: all
presets do, Single Rotation is ESPCA-04cadf and Critters ESPCA-f7ca80. There are 1536 such rules.

Three things in the book do not fit. A figure may hold particles of both automata at once (its
stable patterns do): only those of one fit on a grid here. Its irreversible ESPCAs are not
permutations. And its triangular automata (ETPCA) and the 81-state one need another lattice and
more states.

## Running

```sh
cargo run --release
cargo run --features dev        # dynamic linking: much faster incremental builds while hacking
cargo run -- --help
cargo run --release -p cas-search -- --help    # looking for rules, see below
cargo test --release --workspace
```

Useful flags: `--rule critters` (a preset: `single-rotation`, `critters`, `bbm`, `bounce-gas`, `hpp-gas`,
`tron`, `rotations`, `double-rotation`, `string-thing`, `swap-on-diagonal`), `--rule espca-01c5ef`
(Morita's number of a rule) or `--rule 0,8,4,3,2,5,9,7,1,6,10,11,12,13,14,15` (any reversible table),
`--width 512 --height 512`, `--init blob|soup|empty`,
`--density 0.01`, `--seed 7`, `--threads 4`, `--window 1600x1000`, `--vsync auto|on|off` (see
*Environment notes*).

### Controls

The panel sorts the controls into five cards by what they are about, and each of these aspects
has a colour that comes back wherever it shows up: rose for the **rule** (the rule editor, the
outline of the blocks on the grid), teal for the **world** (the edge of the grid), blue for
**time**, amber for the **pattern** (the cells, the spaceship list, the edge of the grid while it
catches them). The **view** card is grey: the automaton knows nothing of how it is drawn.

Every control shows its shortcut next to its label. Shortcuts are the characters the keys type,
so they follow the keyboard layout; with Ctrl, Alt or Super held they do nothing.

| action | UI | key / mouse |
|--------|----|-------------|
| choose a rule | the rule menu | |
| open / close the rule editor | **Edit** (*Custom…* in the rule menu opens it on your last own rule) | `e` |
| play / pause | **Play** | `space` |
| step one frame back / forward (`stride` generations); hold to repeat | **← step** / **step →** | `←` / `→` (`shift`: a single generation) |
| run backwards in time | *Run backwards in time* | `r` |
| frames per second (0.5 – 240, log scale) | *Frames per second* slider (drag, click, or wheel) | `[` / `]` halve / double |
| generations per frame, i.e. "render every Nth step" (1 – 512) | *Generations per frame* slider (drag, click, or wheel) | `,` / `.` |
| hide vacuum fluctuations | *View* checkbox | `v` |
| cell grid | *Cell grid* | `g` |
| outline the current partition: the 2×2 blocks the next forward step rewrites | *2×2 blocks of the next step* | `p` |
| zoom about the pointer | | mouse wheel, `+` / `−` |
| pan | | right- or middle-drag |
| fit the grid to the window | **Fit** | `f` |
| soup / blob density (0.01 % – 90 %, log scale) | *Density* slider (drag, click, or wheel) | |
| random soup / random blob / clear | **Soup** / **Blob** / **Clear** | `n` / `b` / `c` |
| grid size: 32 to 4096 cells each way; the pattern stays in the middle, what no longer fits is cut off | *Grid size* menu | |
| open border: what reaches the edge of the grid leaves the world, instead of coming back on the other side | *Open border* | `o` |
| catch the spaceships that reach the edge | *Catch spaceships* | `k` |
| show / hide the list of spaceships caught | **Spaceships** | `s` |
| paint | | left-drag: paints the opposite of the first cell touched; with `shift` it erases |

Sliders jump to where you click and then follow the pointer; the wheel over a slider steps it to
its next value. The overlays fade out as you zoom out (they would only be noise once cells are a
few pixels wide), so leaving them on is harmless. The block outline depends only on the generation
(even: blocks aligned with the origin, odd: shifted by one cell diagonally). Stepping forward
rewrites the outlined blocks and then the outline moves on; stepping back restores the previous
picture together with its outline. Note that a lone cell orbits *counter*clockwise even though
every block rotation is clockwise, because consecutive steps use different blocks.

A frame is always a whole number of strides. A frame that did not fit in its update is made up
for by the next ones. When the machine cannot keep up with the requested rate at all, frames are
dropped rather than queued, the window stays responsive, and the status line shows the rate
actually achieved.

Nothing obliges a rule to leave empty blocks empty. Under Critters the empty world is all alive
every other generation; under a random table it is usually some texture that repeats after a few
generations (sixteen at most). So the *pattern* is kept apart from the *vacuum*: the grid stores
how the world differs from the empty world, and steps that difference with the rule taken relative
to its vacuum, which is worked out from the table of any rule. What you see, paint and count is
always the pattern, and a rule taken up mid-run gets the pattern as drawn. Unticking *Hide vacuum
fluctuations* puts the vacuum back under the cells and shows the automaton as it literally is.

### The rule editor

**Edit** (or `e`) opens a second panel showing the rule as its sixteen cases, each a 2×2 block
before → after one step, one rotation orbit per row. The cases the rule changes are highlighted.

A reversible rule is a permutation of the sixteen blocks, and the editor keeps it one: the only edit
is to **swap two outcomes** (click one "after" picture, then another). Every permutation can be
reached that way, and since every intermediate table is itself a valid rule, edits take effect at
once, also while the simulation runs. As soon as the table differs from all presets the rule menu
shows *Custom*; pick a preset and *Custom…* brings your last hand-made rule back.

* **Identity**, **Inverse** (the rule that undoes the current one) and **Random** replace the table.
* **Properties** is what analysis says about the table, each finding in words next to a picture
  of it:
  * *Symmetry*: the turns and mirrors of the square under which the rule looks the same. The
    picture shows a point and its images under those, and the axes of the mirrors; a rule with
    rotations but no mirrors, like Single Rotation, shows as a pinwheel: it has a handedness.
  * *Dead and alive*: whether exchanging the two states turns every run into another run.
  * *Cell count*: whether a pattern keeps its number of cells (possibly only relative to the
    vacuum, as in Critters). The picture shows where the blocks go by their number of cells,
    before across and after upwards: a rule that conserves cells lights the diagonal.
  * *Backwards*: how the rule run backwards relates to the rule: the same (=), its mirror image
    (◧◨), with the two states exchanged (■□), both, or none of it (≠).
  * *Vacuum*: the empty world through the generations of its cycle.

  For a custom rule the main panel says the same in a sentence.
* **Rule string**: the table as text, with Morita's number under it if the rule has one. Type
  or paste a table, a preset name or such a number and it is applied as soon as it is valid
  (otherwise the reason is shown below the field); **Copy** and **Paste** use the system
  clipboard, and `ctrl+a`, `ctrl+c`, `ctrl+v` work in the field. While the field has focus the
  single-key shortcuts are off.

### The edge of the grid, and catching spaceships

The grid is a torus: what leaves on one side comes back on the other. *Open border* (`o`) makes
its edge open space instead. After every step, whatever has reached the first row or the first
column leaves the world (on a torus those two lines are the whole edge): small patterns whole,
anything bigger by the cells that touch the edge. A blob left to itself then evaporates.

*Catch spaceships* (`k`) is a separate matter. While it is on, the small patterns that reach the
edge are taken out of the world and identified, whether the border is open or closed; a closed
border lets everything else pass. The grid is then outlined in the colour of the cells.

* Live cells within four cells of each other are one pattern, as long as there are fewer than 20
  of them; anything bigger is debris.
* To identify a pattern it is run alone on an unbounded plane until it is back in its starting
  shape, which gives its period and how far it has moved by then. What moved is a spaceship; what
  did not, or never came back, is counted under *others*. Ships that fly side by side without
  ever meeting are counted one by one.
* A ship is the same entry whichever way it flew and whenever it was caught: it is filed under
  one form chosen among all phases of its period and all the orientations the rule itself is
  symmetric under, by a fixed order (travelling right, then down; smallest bounding box; cells
  in reading order).

**Spaceships** (`s`) opens the list for the current rule; every rule has a list of its own. A row
shows the pattern, its speed as a fraction of *c* (a cell per generation) with its direction, its
period, its number of cells, how often it was caught, and as a bar its share of all catches. A
click on a row copies the pattern as run-length encoded text (`b2o2$b2o`: `b` dead, `o` alive, `$`
next row, written from a corner of the blocks the next step rewrites). **Clear list** forgets what
was caught under the current rule.

What left or was caught is gone: stepping backwards does not bring it back.

## Searching for rules

`cas-search` looks for interesting rules without opening a window. It puts every rule of a family
through a few trials and writes one line per rule:

```sh
cargo run --release -p cas-search -- --family espca --out searches/espca.tsv
cargo run --release -p cas-search -- --family conserving --out searches/conserving.tsv
cargo run --release -p cas-search -- --rule critters --rule espca-0925bf
```

* **Seeds**: small random patterns (one to six cells) are left alone on an unbounded plane. Each
  comes back to its shape in place (*oscillating*) or elsewhere (*travelling*), flies apart
  (*scattering*), grows without bound (*growing*), or does none of it in time (*undecided*). The
  table has the share of each, the number of different periods among the oscillating ones and
  the longest.
* **Spaceships**: kinds of spaceship slower than light, among the seeds and among what leaves a
  blob through an open border; of the latter also how many were *caught* and how many small
  patterns were *others*.
* **Damage**: one cell of a random soup is flipped; the share that differs, a hundred generations
  on, of the cells the flip could have reached.
* **Remaining**: what is left of the blob.

From these a rule gets a *character*: `explosive` or `growing` (most or some seeds grow without
bound), `spaceships` (something slower than light travels), `gas` (seeds fly apart, at the speed
of light), `frozen` (everything stays put, and so does a change), `confined` (everything stays
put, yet a change spreads). Most rules are explosive. The ones to look at have things that travel
and things that stay, and little damage: a run ends by printing its best rules, ranked by the
lesser of their kinds of spaceship and their periods. (By kinds alone the list would be led by
rules in which a lone cell already flies and nothing stays: cells flying in formation are kinds
too.) A rule found this way is opened with `cargo run --release -- --rule 0,2,8,3,…` or pasted
into the rule field.

Families: `espca`, the rules that look the same after a quarter turn (1536, half a minute);
`conserving`, the rules that keep the number of cells of every block or trade it for the number
of dead cells as Critters does (829 440, half an hour on four threads); `random`. Rules that
differ only by a turn or a mirror, or by a vacuum that flickers and changes nothing else, are
measured once. A search that was interrupted takes up where it stopped if given the same `--out`,
and `--limit` measures a fair sample.

## Test rig

The app can drive itself from a tiny script, which is how the UI gets exercised and screenshotted
during development. The scripts in `rig/` cover the panel, the rule menu and editor, the vacuum
handling and the spaceship catcher; each exits with status 1 at the first expectation that fails:

```sh
cargo run --release -- --script "wait 20; shot start; click PlayPause; wait 60; shot running; quit"
for script in rig/*.cas; do cargo run --release -- --script-file $script || break; done
```

| command | effect |
|---------|--------|
| `wait N` | idle for N frames |
| `shot NAME` | save `shots/NAME.png` (see `--shots`) and wait until it is written |
| `click NAME [DX DY]` | pointer move / press / release on the UI node named NAME, optionally offset from its centre in logical pixels. Nodes: `PlayPause`, `StepBack`, `StepForward`, `Reverse`, `HideVacuum`, `ShowGrid`, `ShowBlocks`, `FitView`, `Soup`, `Blob`, `Clear`, the sliders `Speed`, `Stride`, `Density`, `Grid`; the rule menu `RuleMenu` and its items `RuleItem:<preset id>`, `RuleItemCustom`; `EditRule`; in the editor `Out0` … `Out15` (the outcomes), `RuleIdentity`, `RuleInverse`, `RuleRandom`, `RuleString`, `RuleEspca` (Morita's number under it), `RuleCopy`, `RulePaste`, `EditorClose`; the size menu `GridSize` and its items `GridSize:<side>`; `OpenBorder`, `Catching`, `Spaceships`; in the spaceship list `CatcherCatching`, `Kind0` … (the kinds, in the order they were first caught), `CatcherForget`, `CatcherNote` (the line next to it), `CatcherClose` |
| `move NAME [DX DY]` | just move the pointer there |
| `drag NAME DX DY [left\|right\|middle]` | press at the node's centre, move by (DX, DY), release: drags sliders, paints, pans |
| `hold NAME FRAMES` | keep the left button down on the node for FRAMES frames |
| `scroll NAME LINES` | turn the wheel over the node |
| `key KEY` | press and release a key or chord: `Space`, `ArrowLeft`, `r`, `[`, `=`, `Ctrl+a`, ... |
| `press KEY`, `release KEY` | hold a key across other commands: `press Shift; drag Grid 60 0; release Shift` |
| `type TEXT` | type text into whatever has keyboard focus |
| `paint X Y [on\|off]` | set a cell |
| `place RLE X Y` | put a run-length encoded pattern with its corner at (X, Y) |
| `fit` | fit the view |
| `play`, `pause`, `step N`, `rule RULE` (preset or table), `speed N`, `stride N`, `vacuum on\|off`, `reverse on\|off`, `soup [DENSITY]`, `blob [DENSITY]`, `clear` | direct state changes |
| `expect_gen N`, `expect_cell X Y on\|off`, `expect_population N`, `expect_rule RULE`, `expect_speed N`, `expect_stride N`, `expect_playing on\|off`, `expect_size W H`, `expect_checked NAME on\|off`, `expect_text NAME TEXT`, `expect_caught SHIPS KINDS`, `expect_clipboard TEXT` | fail the run unless the state is as stated (cells and population are the pattern's, without the vacuum) |
| `quit` | exit |

Commands are separated by `;` or newlines, `#` starts a comment. Pointer and keyboard actions are
injected as the messages `bevy_winit` would emit, so they go through picking, focus and the widgets
like real input (the window's own cursor position is left alone; winit would warp the real cursor).
Real input is discarded while a script runs and the window lets the pointer through, so a run does
not depend on what else happens at the machine. Naming a UI node that does not exist fails the run.

Example: `shot a; step 500; step -500; expect_gen 0; shot b` produces two byte-identical PNGs.

## Layout

A cargo workspace of three crates. `cas-core` is the automata without the app and knows nothing
of Bevy; the app at the root and the search program are built on it.

| file | what |
|------|------|
| `crates/cas-core/src/rules.rs` | rules as permutation tables: presets, text form, Morita's numbers, swaps, inverse, the vacuum's cycle and the rule relative to it, analysis (population, symmetry, time reversal), the rule that stands for all that only look different; tests check each preset against its definition and the numbers against the book's statements |
| `…/universe.rs` | the grid and its stepping kernel, the vacuum kept apart from the cells, resizing, what the edge does (open border, catching). Tests hold the kernel against the plain definition of a step and replay the spaceships published with Single Rotation (which pins rotation sense, bit layout and phase to the reference simulator) |
| `…/pattern.rs` | finite patterns on an unbounded plane: what becomes of a pattern left alone, its period, displacement and canonical form, taking apart patterns that only travel together, run-length encoding. Tests use the periods and displacements js-revca's tests give, hold the analysis against the grid, and replay figures of Morita's book (which pins down how his automata lie on the block grid) |
| `…/census.rs` | counting by kind the spaceships a universe caught |
| `…/search.rs` | the trials a rule is put through and its report, the families of rules |
| `crates/cas-search` | the command-line search: families, the table, taking up an interrupted search |
| `src/sim.rs` | the universe in the app: transport and pacing, the settings, the system sets that order a frame; the pacing is tested in a headless app |
| `src/catcher.rs` | the spaceship list: identifies what was caught at the edge within a time budget per frame, for each rule, and shows the panel |
| `src/actions.rs` | everything the user can ask for as one `Action` enum with a single handler; the key table, which also labels the controls; hold-to-repeat stepping; who gets the keyboard |
| `src/view.rs` | the grid node: view state (zoom / pan / fit), the UI material, painting and navigation via picking events |
| `src/grid.wgsl` | the fragment shader: view transform, cell colours, the vacuum under the cells, grid and block overlays |
| `src/ui.rs` | the control panel as cards, one per aspect, and the aspects' colours (Bevy UI + `bevy_feathers` dark theme, `bsn!` scenes); sliders and checkboxes on the headless widgets, widget↔state sync |
| `src/editor.rs` | the rule editor panel: the sixteen cases, swap editing, the properties with their pictures, the rule string and clipboard |
| `src/rig.rs`, `rig/*.cas` | the script-driven test rig and the scripts that exercise the app |

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

* Catching takes patterns out of the world, and so does the open border. A mode that only watches
  them cross the edge (so that time stays reversible) is not there yet, and neither is reseeding
  the grid once a blob has evaporated.
* The size menu offers square grids only; other sizes need `--width` and `--height`.
* Catching with a closed border on a grid full of soup is slow, six to nine times a plain step at
  256×256: everything on the edge is debris, and it is looked at again after every generation.
* Patterns that leave together and then part ways (a spaceship next to debris, two ships on
  different courses) are one pattern to the catcher. It never comes back to its shape, so it is
  counted as one of the *others* and its ships are missed, which happens a lot in a gas like the
  billiard ball machine.
* Spaceships that follow each other closely are one pattern to the catcher as well, and a long
  train of them is debris. The gun of ESPCA-09457f fires such trains: an open border wears them
  down cell by cell, and what is left of a spaceship blows up. Watch it with the border closed.
* The spaceship list lives as long as the program; it is not saved.
* The paused app still redraws every frame; Bevy's reactive update mode would let it idle.
* Painting and wheel zoom assume a `UiScale` of 1.
* Under GNOME the clipboard goes through XWayland; pasting into a native Wayland application is
  untested.
* A scripted window still takes the keyboard focus when it opens. Its input is discarded, but
  keystrokes meant for the window behind it are lost while it is in front.
* Buttons give up the keyboard focus so that Space and the arrows always drive the simulation,
  which also means there is no Tab navigation.

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
* The book in `docs/` is git-ignored.
