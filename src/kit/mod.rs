//! The kit: what the panels are made of.
//!
//! Only Bevy is used here, nothing of the app and nothing of the automata: a panel says what
//! it shows, and how that looks is said here, once.
//!
//! The kit does not set itself up. Whoever uses it installs [`theme`] and the icon font
//! ([`icons::IconsPlugin`]), and runs the systems that keep its elements looking as they
//! should: [`style_toggles`], [`style_sliders`], [`show_scrollbars`] and [`fit_menus`].

mod aspect;
mod cards;
mod controls;
pub mod icons;
mod text;

pub(crate) use aspect::{ALIVE, Aspect, DEAD, theme};
pub(crate) use cards::{
    AXES, GLYPH, GUTTER, ORBIT, card, panel_title, section, side_panel, tile, tile_label, tile_value,
};
pub(crate) use controls::{
    button, checkbox, field_frame, fit_menus, icon_button, icon_button_marked, menu_heading, show_scrollbars, slider,
    style_sliders, style_toggles,
};
pub(crate) use text::{caption, group_digits, key_hint, readout};
