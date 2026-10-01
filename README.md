# cas

A sandbox for exploring **reversible block cellular automata**, built on [Bevy](https://bevy.org) 0.19.

Only *block* CAs on the **Margolus neighbourhood** are supported for now: the grid is cut into
2×2 blocks, every block is rewritten by a lookup table, and the partition shifts by one cell
diagonally between generations. Because each table is a permutation of the 16 block states, every
rule has an exact inverse and time can be run backwards.

Rules so far:

| rule | definition |
|------|------------|
| **Single rotation** | blocks with exactly one live cell rotate 90° clockwise; everything else stays ([dmishin](https://dmishin.blogspot.com/2013/11/the-single-rotation-rule-remarkably.html)) |
| **Critters** | 0, 1 or 4 live cells: invert the block; 2: keep it; 3: invert and rotate 180° ([Wikipedia](https://en.wikipedia.org/wiki/Critters_(cellular_automaton))) |

## Running

```sh
cargo run --release
cargo run --features dev        # dynamic linking: much faster incremental builds while hacking
cargo run -- --help
```

Useful flags: `--rule critters`, `--width 512 --height 512`, `--init blob|soup|empty`,
`--density 0.01`, `--seed 7`, `--window 1600x1000`, `--vsync auto|on|off` (see *Environment notes*).

### Controls

| action | UI | key / mouse |
|--------|----|-------------|
| play / pause | **Play** | `space` |
| step one frame back / forward (`stride` generations); hold to repeat | **← step** / **step →** | `←` / `→` (`shift`: a single generation) |
| run backwards in time | *Run backwards in time* | `r` |
| frames per second (0.5 – 240, log scale) | *Frames per second* slider | `[` / `]` halve / double |
| generations per frame, i.e. "render every Nth step" (1 – 512) | *Generations per frame* slider | `,` / `.` |
| hide vacuum fluctuations (draw the complement on odd generations) | *View* checkbox | `v` |
| cell grid | *Cell grid* | `g` |
| outline the current partition: the 2×2 blocks a forward step rewrites next | *2×2 blocks a forward step rotates* | `p` |
| zoom about the pointer | | mouse wheel, `+` / `−` |
| pan | | right- or middle-drag |
| fit the grid to the window | **Fit** | `f` |
| soup / blob density (0.01 % – 90 %, log scale) | *Density* slider | |
| random soup / random blob / clear | **Soup** / **Blob** / **Clear** | `n` / `b` / `c` |
| paint | | left-drag: paints the opposite of the first cell touched; with `shift` it erases |

Sliders jump to where you click and then follow the pointer. The overlays fade out as you zoom
out (they would only be noise once cells are a few pixels wide), so leaving them on is harmless.
The block outline depends only on the generation (even: blocks aligned with the origin, odd:
shifted by one cell diagonally). Stepping forward rewrites the outlined blocks and then the outline
moves on; stepping back restores the previous picture together with its outline. Note that a lone
cell orbits *counter*clockwise even though every block rotation is clockwise, because consecutive
steps use different blocks.

The vacuum of Critters flips on every step (an empty block becomes a full one), so without
*hide vacuum fluctuations* the whole picture inverts on odd generations. The cells themselves are
never altered by the option; only the picture is.

## Test rig

The app can drive itself from a tiny script, which is how the UI gets exercised and screenshotted
during development:

```sh
cargo run -- --script "wait 20; shot start; click PlayPause; wait 60; shot running; quit"
cargo run -- --script-file tour.cas --shots shots/
```

| command | effect |
|---------|--------|
| `wait N` | idle for N frames |
| `shot NAME` | save `shots/NAME.png` and wait until it is written |
| `click NAME [DX DY]` | pointer move / press / release on the UI node named NAME, optionally offset from its centre in logical pixels. Nodes: `PlayPause`, `StepBack`, `StepForward`, `Reverse`, `HideVacuum`, `ShowGrid`, `ShowBlocks`, `FitView`, `Soup`, `Blob`, `Clear`, `RuleSingleRotation`, `RuleCritters`, the sliders `Speed`, `Stride`, `Density`, and `Grid` |
| `move NAME [DX DY]` | just move the pointer there |
| `drag NAME DX DY [left\|right\|middle]` | press at the node's centre, move by (DX, DY), release: drags sliders, paints, pans |
| `hold NAME FRAMES` | keep the left button down on the node for FRAMES frames |
| `scroll NAME LINES` | turn the wheel over the node |
| `key KEY` | press and release a key: `Space`, `ArrowLeft`, `r`, `[`, `=`, ... |
| `paint X Y [on\|off]` | set a cell |
| `fit` | fit the view |
| `play`, `pause`, `step N`, `rule NAME`, `speed N`, `stride N`, `vacuum on\|off`, `reverse on\|off`, `soup [DENSITY]`, `blob [DENSITY]`, `clear` | direct state changes |
| `expect_gen N` | exit with status 1 unless the generation counter is N |
| `expect_cell X Y on\|off` | exit with status 1 unless the cell has that state |
| `quit` | exit |

Commands are separated by `;` or newlines, `#` starts a comment. Pointer and keyboard actions are
injected as the messages `bevy_winit` would emit, so they go through picking, focus and the widgets
like real input (the window's own cursor position is left alone; winit would warp the real cursor).

Examples: `step 500; shot a; step -500; expect_gen 0; shot b` produces two byte-identical PNGs;
`clear; drag Grid 60 0; expect_cell 140 128 on` checks that a paint stroke lands where it should.

## Layout

| file | what |
|------|------|
| `src/rules.rs` | block rules as 16-entry tables, inverse by inverting the permutation, published-table tests |
| `src/sim.rs` | the toroidal grid, forward/backward stepping (one rayon task per row), transport state; tests replay the spaceships published with Single Rotation, which pins rotation sense, bit layout and phase to the reference simulator |
| `src/view.rs` | the grid node: view state (zoom / pan / fit), the UI material, painting and navigation via picking events |
| `src/grid.wgsl` | the fragment shader: view transform, cell colours, vacuum inversion, grid and block overlays |
| `src/ui.rs` | the control panel (Bevy UI + `bevy_feathers` dark theme, `bsn!` scenes), custom sliders on the headless `Slider` widget, shortcuts, hold-to-repeat stepping, widget↔state sync |
| `src/rig.rs` | the script-driven test rig |

Stepping backwards from generation *g* applies the inverse table with the partition the forward
step *g−1 → g* used (even generations: blocks aligned with the origin; odd: shifted by (1, 1)).

Drawing: the cell array is uploaded as-is into an `R8Uint` texture (one byte per cell, a plain
`memcpy` when the universe changes) and a UI material's fragment shader does everything else, so
zooming, panning and toggling overlays cost nothing on the CPU. The simulation step (bytes, one
rayon task per row) will be the bottleneck long before drawing is; bit-packing is the next lever.

## Environment notes

* Bevy comes from the sibling checkout `../bevy` (tag `v0.19.1`); its `examples/` are the API
  reference. Bevy's features are enumerated explicitly in `Cargo.toml` (no 3D, audio or gamepads).
  Native Wayland is the crate's default `wayland` feature and needs `libwayland-dev` at build time;
  `cargo build --no-default-features` falls back to winit's X11 backend (XWayland).
* Vsync: `--vsync auto` (the default) waits for vsync only for native Wayland windows, where it
  works fine on both the NVIDIA and the Intel adapter (~60 fps; ~150 fps with `--vsync off`).
  Under XWayland `PresentMode::AutoVsync` stalls the frame loop to about 1 fps, hence the default.
* The book in `docs/` is git-ignored.
