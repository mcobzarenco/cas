//! Running the universe in the app: the transport and its pacing, the settings, and the order
//! of a frame. The universe itself is `cas_core`'s.

use std::time::Duration;

use bevy::{platform::time::Instant, prelude::*};
use cas_core::{rules::BlockRule, universe::Universe};

/// The order of a frame in `Update`: input changes the world and the transport, the simulation
/// steps, then everything that shows the result catches up.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SimSystems {
    Input,
    Step,
    Present,
}

/// Transport state. `speed` is frames per second; each frame advances `stride` generations, so
/// "render every Nth step" is simply `stride = N`.
#[derive(Resource, Clone, Debug)]
pub struct Playback {
    pub playing: bool,
    pub reverse: bool,
    pub speed: f32,
    pub stride: u32,
    /// How long one update may spend stepping. Frames that do not fit wait for the next
    /// update, or are dropped if too many are waiting: see [`advance`].
    pub budget: Duration,
}

impl Playback {
    pub const MIN_SPEED: f32 = 0.5;
    pub const MAX_SPEED: f32 = 240.0;
    pub const MAX_STRIDE: u32 = 512;

    /// `+1` forwards, `-1` backwards.
    pub fn direction(&self) -> i64 {
        if self.reverse { -1 } else { 1 }
    }
}

impl Default for Playback {
    fn default() -> Self {
        Self {
            playing: false,
            reverse: false,
            speed: 30.0,
            stride: 1,
            budget: Duration::from_millis(12),
        }
    }
}

/// How fast the simulation really runs when it cannot keep up with [`Playback`].
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct Pace {
    /// Generations per second, measured; `None` while the requested rate is met.
    pub achieved: Option<f32>,
}

/// Everything that is not transport: how the world is drawn and seeded.
#[derive(Resource, Clone, Debug)]
pub struct Settings {
    /// Draw the cells as they differ from the vacuum, which is how they are stored. Unhidden,
    /// the true cells are drawn: for a rule like Critters the whole picture then flickers.
    pub hide_vacuum: bool,
    /// Live-cell probability of a random soup.
    pub density: f32,
    /// Outline the cells (when zoomed in far enough to see them).
    pub show_grid: bool,
    /// Outline the current partition: the 2×2 blocks a forward step rewrites next.
    pub show_blocks: bool,
}

impl Settings {
    pub const MIN_DENSITY: f32 = 0.0001;
    pub const MAX_DENSITY: f32 = 0.9;
}

/// Run condition: has the rule changed since this condition last looked? The universe as a
/// whole counts as changed on every step, so its own change flag cannot tell.
pub fn rule_changed(universe: Res<Universe>, mut seen: Local<Option<BlockRule>>) -> bool {
    let changed = seen.as_ref() != Some(universe.rule());
    if changed {
        *seen = Some(universe.rule().clone());
    }
    changed
}

pub struct SimPlugin;

impl Plugin for SimPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Playback>()
            .init_resource::<Pace>()
            .configure_sets(
                Update,
                (SimSystems::Input, SimSystems::Step, SimSystems::Present).chain(),
            )
            .add_systems(Update, advance.in_set(SimSystems::Step));
    }
}

/// The rate actually achieved is measured over windows of this length.
const PACE_WINDOW: Duration = Duration::from_millis(500);

#[derive(Default)]
struct PaceMeter {
    since: Option<Instant>,
    generations: u64,
    lagging: bool,
}

/// Runs the simulation while playing: one frame of `stride` generations every `1 / speed`
/// seconds. Frames are never split, so the picture always shows a multiple of the stride.
/// An update steps for no longer than `budget`, give or take a frame. The frames that did
/// not fit stay owed, so that a slow update is made up for by the next ones; but a machine
/// that cannot keep up must not fall ever further behind, so no more stay owed than an update
/// brings in. The rest is dropped: the simulation slows down instead of freezing the window.
fn advance(
    time: Res<Time>,
    playback: Res<Playback>,
    mut universe: ResMut<Universe>,
    mut pace: ResMut<Pace>,
    mut owed: Local<f64>,
    mut meter: Local<PaceMeter>,
) {
    if !playback.playing {
        *owed = 0.0;
        *meter = PaceMeter::default();
        pace.set_if_neq(Pace::default());
        return;
    }

    let came_due = time.delta_secs_f64() * playback.speed as f64;
    *owed += came_due;
    let due = owed.floor() as u32;
    let started = Instant::now();
    let mut done = 0;
    while done < due {
        universe.step_by(playback.stride as i64 * playback.direction());
        done += 1;
        if started.elapsed() >= playback.budget {
            break;
        }
    }
    *owed -= done as f64;
    let backlog = came_due.max(1.0);
    let lagging = *owed > backlog;
    if lagging {
        *owed = backlog;
    }

    meter.generations += done as u64 * playback.stride as u64;
    meter.lagging |= lagging;
    let elapsed = meter.since.get_or_insert(started).elapsed();
    if elapsed >= PACE_WINDOW {
        let rate = meter.generations as f32 / elapsed.as_secs_f32();
        pace.set_if_neq(Pace {
            achieved: meter.lagging.then_some(rate),
        });
        *meter = PaceMeter::default();
    }
}

/// The pacing system, driven by a headless app whose clock advances a quarter second per update.
#[cfg(test)]
mod pacing {
    use super::*;
    use bevy::time::TimeUpdateStrategy;

    const TICK: Duration = Duration::from_millis(250);

    fn app(playback: Playback) -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, SimPlugin))
            .insert_resource(TimeUpdateStrategy::ManualDuration(TICK))
            .insert_resource(Universe::new(8, 8, "single-rotation".parse().unwrap()))
            .insert_resource(playback);
        // The first update only starts the clock.
        app.update();
        app
    }

    fn generation(app: &App) -> i64 {
        app.world().resource::<Universe>().generation
    }

    #[test]
    fn a_frame_is_a_whole_stride() {
        // 8 frames per second for a quarter second: two frames of three generations.
        let mut app = app(Playback { playing: true, speed: 8.0, stride: 3, ..default() });
        app.update();
        assert_eq!(generation(&app), 6);
        app.update();
        assert_eq!(generation(&app), 12);
        assert_eq!(app.world().resource::<Pace>().achieved, None);
    }

    #[test]
    fn slow_speeds_wait_for_their_frame() {
        let mut app = app(Playback { playing: true, speed: 1.0, stride: 5, ..default() });
        for _ in 0..3 {
            app.update();
            assert_eq!(generation(&app), 0);
        }
        app.update();
        assert_eq!(generation(&app), 5);
    }

    #[test]
    fn reverse_and_pause() {
        let mut app = app(Playback { playing: true, reverse: true, speed: 4.0, ..default() });
        app.update();
        assert_eq!(generation(&app), -1);
        app.world_mut().resource_mut::<Playback>().playing = false;
        app.update();
        assert_eq!(generation(&app), -1);
    }

    fn set_budget(app: &mut App, budget: Duration) {
        app.world_mut().resource_mut::<Playback>().budget = budget;
    }

    #[test]
    fn a_slow_update_is_made_up_for() {
        // Two frames are due per update. With no budget only the first one runs ...
        let playback = Playback { playing: true, speed: 8.0, budget: Duration::ZERO, ..default() };
        let mut app = app(playback);
        app.update();
        assert_eq!(generation(&app), 1);
        // ... and the other is still owed when there is time again.
        set_budget(&mut app, Duration::from_secs(1));
        app.update();
        assert_eq!(generation(&app), 4);
        assert_eq!(app.world().resource::<Pace>().achieved, None);
    }

    #[test]
    fn work_far_over_budget_is_dropped_not_owed() {
        // 60 frames are due per update, but with no budget only the first one runs, and the
        // stride stays whole.
        let playback = Playback {
            playing: true,
            speed: 240.0,
            stride: 7,
            budget: Duration::ZERO,
            ..default()
        };
        let mut app = app(playback);
        for update in 1..=4 {
            app.update();
            assert_eq!(generation(&app), 7 * update);
        }
        // Of the 236 frames missed, one update's worth is still owed; the rest did not pile up.
        set_budget(&mut app, Duration::from_secs(1));
        app.update();
        assert_eq!(generation(&app), 7 * (4 + 60 + 60));
    }
}
