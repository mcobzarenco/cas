//! cas — a sandbox for exploring reversible block cellular automata.

// Bevy systems take all their inputs as parameters, and queries spell out what they touch in
// their types; the usual lint thresholds don't fit either (bevy itself allows both).
#![allow(clippy::too_many_arguments, clippy::type_complexity)]

mod actions;
mod catcher;
mod editor;
mod pattern;
mod rig;
mod rules;
mod sim;
mod ui;
mod view;

use std::path::PathBuf;

use bevy::{
    prelude::*,
    window::{CursorOptions, PresentMode, WindowResolution},
};
use clap::{Parser, ValueEnum};

use crate::{
    rules::BlockRule,
    sim::{Rng, Settings, Universe},
};

/// The cells are drawn from a single texture, and this is what GPUs commonly allow.
const MAX_GRID_SIDE: usize = 16384;

/// A sandbox for reversible block cellular automata on the Margolus neighbourhood.
#[derive(Parser, Debug)]
#[command(name = "cas", version, about)]
struct Args {
    /// Grid width in cells (must be even).
    #[arg(long, default_value_t = 256)]
    width: usize,
    /// Grid height in cells (must be even).
    #[arg(long, default_value_t = 256)]
    height: usize,
    /// Rule to start with: a preset (single-rotation, critters, bbm, bounce-gas, hpp-gas, tron,
    /// rotations, double-rotation, string-thing, swap-on-diagonal) or a table of 16 block
    /// states such as 0,2,8,3,1,5,6,7,4,9,10,11,12,13,14,15.
    #[arg(long, default_value = "single-rotation")]
    rule: BlockRule,
    /// Initial pattern: a random square in the middle, a uniform random soup, or nothing.
    #[arg(long, value_enum, default_value = "blob")]
    init: Init,
    /// Live-cell probability of the initial pattern.
    #[arg(long, default_value_t = 0.3)]
    density: f32,
    /// Seed of the random soup.
    #[arg(long, default_value_t = 42)]
    seed: u64,
    /// Threads for stepping large grids. The step is bound by memory, not arithmetic, so a
    /// handful is as fast as all of them.
    #[arg(long, default_value_t = default_threads())]
    threads: usize,
    /// Window size as WIDTHxHEIGHT.
    #[arg(long, default_value = "1440x900")]
    window: String,
    /// Wait for vsync when presenting. `auto` turns it on for native Wayland windows and off
    /// otherwise, because under XWayland vsync stalls the frame loop to ~1 fps.
    #[arg(long, value_enum, default_value = "auto")]
    vsync: Vsync,
    /// Test-rig script; see `src/rig.rs` for the commands.
    #[arg(long, conflicts_with = "script_file")]
    script: Option<String>,
    /// Read the test-rig script from a file instead.
    #[arg(long)]
    script_file: Option<PathBuf>,
    /// Where the test rig saves screenshots.
    #[arg(long, default_value = "shots")]
    shots: PathBuf,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Init {
    Blob,
    Soup,
    Empty,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Vsync {
    Auto,
    On,
    Off,
}

impl Vsync {
    fn present_mode(self) -> PresentMode {
        let on = match self {
            Vsync::On => true,
            Vsync::Off => false,
            Vsync::Auto => {
                cfg!(feature = "wayland")
                    && std::env::var_os("WAYLAND_DISPLAY").is_some_and(|d| !d.is_empty())
            }
        };
        if on {
            PresentMode::AutoVsync
        } else {
            PresentMode::AutoNoVsync
        }
    }
}

fn default_threads() -> usize {
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    (cores / 2).clamp(1, 8)
}

fn main() -> AppExit {
    let args = Args::parse();
    let (window_width, window_height) = parse_window(&args.window).unwrap_or_else(|e| fail(&e));
    for side in [args.width, args.height] {
        if side < 2 || side % 2 != 0 {
            fail("the grid needs even, positive dimensions (Margolus blocks are 2×2)");
        }
        if side > MAX_GRID_SIDE {
            fail(&format!("the grid can be at most {MAX_GRID_SIDE} cells wide and high"));
        }
    }
    if !(0.0..=1.0).contains(&args.density) {
        fail("--density is a probability between 0 and 1");
    }
    rayon::ThreadPoolBuilder::new()
        .num_threads(args.threads.max(1))
        .build_global()
        .expect("nothing has used the thread pool yet");
    let script = match (&args.script, &args.script_file) {
        (Some(script), _) => Some(script.clone()),
        (None, Some(path)) => Some(std::fs::read_to_string(path).unwrap_or_else(|e| {
            fail(&format!("cannot read {}: {e}", path.display()))
        })),
        (None, None) => None,
    };
    let script = script.map(|script| {
        rig::parse_script(&script).unwrap_or_else(|e| fail(&format!("bad script: {e}")))
    });

    let mut rng = Rng::new(args.seed);
    let mut universe = Universe::new(args.width, args.height, args.rule);
    match args.init {
        Init::Blob => universe.randomize_blob(args.density, &mut rng),
        Init::Soup => universe.randomize(args.density, &mut rng),
        Init::Empty => {}
    }

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "cas — reversible cellular automata".into(),
            resolution: WindowResolution::new(window_width, window_height),
            present_mode: args.vsync.present_mode(),
            ..default()
        }),
        // A scripted run is not for clicking on: let the real pointer through to whatever
        // is behind the window.
        primary_cursor_options: Some(CursorOptions {
            hit_test: script.is_none(),
            ..default()
        }),
        ..default()
    }))
    .insert_resource(ClearColor(view::BACKGROUND))
    .insert_resource(universe)
    .insert_resource(rng)
    .insert_resource(Settings {
        hide_vacuum: true,
        density: args
            .density
            .clamp(Settings::MIN_DENSITY, Settings::MAX_DENSITY),
        show_grid: true,
        show_blocks: true,
    })
    .add_plugins((
        sim::SimPlugin,
        view::ViewPlugin,
        actions::ActionsPlugin,
        ui::UiPlugin,
        editor::EditorPlugin,
        catcher::CatcherPlugin,
    ));
    if let Some(script) = script {
        app.add_plugins(rig::RigPlugin {
            script,
            dir: args.shots,
        });
    }
    // The exit code matters to scripts: the rig fails with a non-zero status.
    app.run()
}

fn parse_window(spec: &str) -> Result<(u32, u32), String> {
    let (w, h) = spec
        .split_once(['x', 'X', '×'])
        .ok_or_else(|| format!("window size {spec:?} is not WIDTHxHEIGHT"))?;
    let parse = |s: &str| {
        s.trim()
            .parse::<u32>()
            .map_err(|e| format!("window size {spec:?}: {e}"))
    };
    Ok((parse(w)?, parse(h)?))
}

fn fail(message: &str) -> ! {
    eprintln!("cas: {message}");
    std::process::exit(2)
}
