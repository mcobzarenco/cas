//! The kit: what the panels are made of.
//!
//! Only Bevy is used here, nothing of the app and nothing of the automata: a panel says what
//! it shows, and how that looks is said here, once.
//!
//! [`KitPlugin`] sets it up: Feathers' widgets in the kit's theme, the icon font, and the
//! systems that keep the elements looking as they should, which run in [`KitSystems`].

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

pub(crate) use aspect::{ALIVE, Aspect, BLOCKS, DEAD};
pub(crate) use cards::{
    AXES, GLYPH, GUTTER, ORBIT, card, panel_title, section, side_panel, tile, tile_label, tile_value,
};
pub(crate) use controls::{
    Scrolls, Sign, button, checkbox, chip, chip_box, field_frame, icon_button, icon_button_marked, menu_heading,
    scrolling, slider,
};
pub(crate) use lists::{CELLS_COLUMN, COLUMN_GAP, Flown, PERIOD_COLUMN, PICTURE, dial, glow, picture};
pub(crate) use text::{caption, fitting, group_digits, heading, key_hint, mono, number, readout};

/// Sets the kit up: Feathers' widgets in the kit's theme, the icon font, and the systems that
/// keep the elements looking as they should.
pub(crate) struct KitPlugin;

/// When, in `Update`, the kit brings its elements in line with what they stand for: a
/// checkbox with its `Checked`, a slider with its value, a scrollbar with what there is to
/// scroll. Whatever changes those comes before.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct KitSystems;

impl Plugin for KitPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((FeathersPlugins, icons::plugin, controls::plugin)).insert_resource(UiTheme(aspect::theme()));
    }
}
