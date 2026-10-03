//! The icons of the interface: glyphs of the Phosphor icon font (bold, MIT), which lies in
//! `assets/fonts` and is built into the program, as the shader is, so that it is there
//! wherever the program runs.

use std::path::{Path, PathBuf};

use bevy::{
    asset::io::embedded::EmbeddedAssetRegistry,
    prelude::*,
    text::{FontSourceTemplate, FontWeight},
};

/// Where the font is found once it is registered.
pub const FONT: &str = "embedded://cas/fonts/Phosphor-Bold.ttf";

/// A block that becomes a turn of itself.
pub const TURN: &str = "\u{e036}";
/// A mirror, left to right and top to bottom; turned an eighth, across a diagonal.
pub const MIRROR: &str = "\u{ed6a}";
pub const FLIP: &str = "\u{ed6c}";
/// What patterns keep: their cells, a weight, the parity of their cells, their momentum.
pub const CELLS: &str = "\u{e464}";
pub const WEIGHT: &str = "\u{e750}";
pub const PARITY: &str = "\u{e3d8}";
pub const MOMENTUM: &str = "\u{e528}";
/// Patterns superpose.
pub const LINEAR: &str = "\u{e3d6}";
/// Forwards as backwards.
pub const INVERSE: &str = "\u{e0a0}";
/// The two states, exchanged.
pub const STATES: &str = "\u{e18c}";
/// The empty world.
pub const EMPTY: &str = "\u{edbc}";
pub const EQUAL: &str = "\u{e21c}";
pub const UNEQUAL: &str = "\u{eda6}";
/// A closer look.
pub const LOOK: &str = "\u{e30c}";
/// What opens, and one less, one more.
pub const OPENS: &str = "\u{e13a}";
pub const LESS: &str = "\u{e32a}";
pub const MORE: &str = "\u{e3d4}";

pub struct IconsPlugin;

impl Plugin for IconsPlugin {
    fn build(&self, app: &mut App) {
        // Not `embedded_asset!`: that is for files next to the source, and names them after
        // where they lie.
        app.world().resource::<EmbeddedAssetRegistry>().insert_asset(
            PathBuf::from("assets/fonts/Phosphor-Bold.ttf"),
            Path::new("cas/fonts/Phosphor-Bold.ttf"),
            include_bytes!("../assets/fonts/Phosphor-Bold.ttf") as &'static [u8],
        );
    }
}

/// An icon, so many pixels high.
pub fn icon(glyph: &'static str, size: f32, color: Color) -> impl Scene {
    bsn! {
        Text(glyph)
        TextFont {
            font: FontSourceTemplate::Handle(FONT),
            font_size: FontSize::Px(size),
            weight: FontWeight::NORMAL,
        }
        TextColor(color)
    }
}
