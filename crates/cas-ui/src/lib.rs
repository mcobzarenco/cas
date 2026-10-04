//! The kit: what the panels of cas are made of.
//!
//! Colours, text in the sizes it comes in, cards, side panels and tiles, the controls, the
//! rows of lists and the icons, as scenes for Bevy UI on top of Feathers' widgets. Only Bevy
//! is used here, nothing of the app and nothing of the automata: a panel says what it shows,
//! and how that looks is said here, once.
//!
//! [`KitPlugin`] sets it up: Feathers' widgets in the kit's theme, the icon font, and the
//! systems that keep the elements looking as they should, which run in [`KitSystems`]. An
//! element is a function that returns a scene; what it stands for and what it does are put
//! on it by whoever makes it:
//!
//! ```no_run
//! use bevy::{prelude::*, ui_widgets::Activate};
//! use cas_ui::{Aspect, KitPlugin, button, caption, card, checkbox};
//!
//! fn main() {
//!     App::new().add_plugins((DefaultPlugins, KitPlugin)).add_systems(Startup, spawn).run();
//! }
//!
//! fn spawn(mut commands: Commands) {
//!     commands.spawn(Camera2d);
//!     commands.spawn_scene(card(
//!         Aspect::View,
//!         bsn_list![],
//!         bsn_list![
//!             caption("How the grid is drawn."),
//!             checkbox("Cell grid", "ShowGrid", Aspect::View, "g"),
//!             (button("Fit") on(|_: On<Activate>| info!("fit"))),
//!         ],
//!     ));
//! }
//! ```

// Queries spell out what they touch in their types; the usual threshold doesn't fit (bevy
// itself allows this).
#![allow(clippy::type_complexity)]

mod aspect;
mod cards;
mod controls;
pub mod icons;
mod lists;
mod text;

use bevy::{
    feathers::{FeathersPlugins, theme::UiTheme},
    prelude::*,
};

pub use aspect::{ALIVE, Aspect, BLOCKS, DEAD};
pub use cards::{
    AXES, GLYPH, GUTTER, ORBIT, card, panel_header, panel_title, section, side_panel, tile, tile_label, tile_picture,
    tile_value,
};
pub use controls::{
    Scrolls, Sign, button, button_marked, check, checkbox, chip, chip_box, chip_marked, field_frame, icon_button,
    icon_button_marked, keyed_button, menu_heading, scrolling, slider,
};
pub use lists::{
    CELLS_COLUMN, COLUMN_GAP, Flown, PERIOD_COLUMN, PICTURE, dial, glow, list_row, picture, share_bar, share_bar_marked,
};
pub use text::{caption, fitting, group_digits, heading, key_hint, mono, number, readout, sans, section_title};

/// Sets the kit up: Feathers' widgets in the kit's theme, the icon font, and the systems that
/// keep the elements looking as they should.
pub struct KitPlugin;

/// When, in `Update`, the kit brings its elements in line with what they stand for: a
/// checkbox with its `Checked`, a slider with its value, a scrollbar with what there is to
/// scroll. Whatever changes those comes before.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KitSystems;

impl Plugin for KitPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((FeathersPlugins, icons::plugin, controls::plugin, lists::plugin))
            .insert_resource(UiTheme(aspect::theme()));
    }
}
