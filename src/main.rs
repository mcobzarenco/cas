//! cas — a sandbox for exploring reversible block cellular automata.

// Bevy systems take all their inputs as parameters, and queries spell out what they touch in
// their types; the usual lint thresholds don't fit either (bevy itself allows both).
#![allow(clippy::too_many_arguments, clippy::type_complexity)]

mod rig;
mod rules;
mod sim;
mod ui;
mod view;

use std::path::PathBuf;

use bevy::{
    prelude::*,
    window::{PresentMode, WindowResolution},
};
use clap::{Parser, ValueEnum};

use crate::{
    rules::RuleKind,
    sim::{Playback, Rng, Settings, Universe},
};

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
    /// Rule to start with: single-rotation or critters.
    #[arg(long, default_value = "single-rotation")]
    rule: RuleKind,
    /// Initial pattern: a random square in the middle, a uniform random soup, or nothing.
    #[arg(long, value_enum, default_value = "blob")]
    init: Init,
    /// Live-cell probability of the initial pattern.
    #[arg(long, default_value_t = 0.3)]
    density: f32,
    /// Seed of the random soup.
    #[arg(long, default_value_t = 42)]
    seed: u64,
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

fn main() {
    let args = Args::parse();
    let (window_width, window_height) = parse_window(&args.window).unwrap_or_else(|e| fail(&e));
    if args.width % 2 != 0 || args.height % 2 != 0 || args.width < 2 || args.height < 2 {
        fail("the grid needs even, positive dimensions (Margolus blocks are 2×2)");
    }
    let script = match (&args.script, &args.script_file) {
        (Some(script), _) => Some(script.clone()),
        (None, Some(path)) => Some(std::fs::read_to_string(path).unwrap_or_else(|e| {
            fail(&format!("cannot read {}: {e}", path.display()))
        })),
        (None, None) => None,
    };
    let actions = script.map(|script| {
        rig::parse_script(&script).unwrap_or_else(|e| fail(&format!("bad script: {e}")))
    });

    let mut rng = Rng::new(args.seed);
    let mut universe = Universe::new(args.width, args.height, args.rule.rule());
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
        ..default()
    }))
    .insert_resource(ClearColor(Color::srgb_u8(0x1F, 0x1F, 0x24)))
    .insert_resource(universe)
    .insert_resource(rng)
    .insert_resource(Playback::default())
    .insert_resource(Settings {
        hide_vacuum: true,
        density: args
            .density
            .clamp(Settings::MIN_DENSITY, Settings::MAX_DENSITY),
        show_grid: true,
        show_blocks: true,
    })
    .add_plugins((sim::SimPlugin, view::ViewPlugin, ui::UiPlugin));
    if let Some(actions) = actions {
        app.add_plugins(rig::RigPlugin {
            actions,
            dir: args.shots,
        });
    }
    app.run();
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
