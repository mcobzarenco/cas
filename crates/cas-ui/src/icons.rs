//! The icons of the interface: glyphs of the Phosphor icon font (bold, MIT), which lies in
//! the crate's `assets/fonts` and is built into the program, so that it is there wherever the
//! program runs.

use std::path::{Path, PathBuf};

use bevy::{
    asset::io::embedded::EmbeddedAssetRegistry,
    prelude::*,
    text::{FontSourceTemplate, FontWeight},
};

/// Where the font is found once it is registered.
pub const FONT: &str = "embedded://cas_ui/fonts/Phosphor-Bold.ttf";

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

/// What a pattern is: one that keeps still, one that goes round, one that flies (its nose
/// up, to be turned the way the pattern goes), one that grows, one that flies apart, and one
/// that had not made up its mind.
pub const STILL: &str = "\u{e5aa}";
pub const OSCILLATES: &str = "\u{ea9a}";
pub const SHIP: &str = "\u{e3fc}";
pub const GROWS: &str = "\u{e4ae}";
pub const APART: &str = "\u{e0a4}";
pub const UNDECIDED: &str = "\u{e2b2}";
/// How long a pattern takes to be back, and how much of it changes on the way.
pub const PERIOD: &str = "\u{e492}";
pub const CHANGES: &str = "\u{e2de}";
/// The eight ways to go, clockwise from straight up.
pub const WAYS: [&str; 8] =
    ["\u{e08e}", "\u{e092}", "\u{e06c}", "\u{e042}", "\u{e03e}", "\u{e040}", "\u{e058}", "\u{e090}"];
/// What holds a rule in the rule menu.
pub const PIN: &str = "\u{e3e2}";
/// The mark of a pattern that is kept.
pub const KEEP: &str = "\u{e0ea}";

/// Every icon with its name, as the gallery shows them.
pub const ALL: [(&str, &str); 35] = [
    ("TURN", TURN),
    ("MIRROR", MIRROR),
    ("FLIP", FLIP),
    ("CELLS", CELLS),
    ("WEIGHT", WEIGHT),
    ("PARITY", PARITY),
    ("MOMENTUM", MOMENTUM),
    ("LINEAR", LINEAR),
    ("INVERSE", INVERSE),
    ("STATES", STATES),
    ("EMPTY", EMPTY),
    ("EQUAL", EQUAL),
    ("UNEQUAL", UNEQUAL),
    ("LOOK", LOOK),
    ("OPENS", OPENS),
    ("LESS", LESS),
    ("MORE", MORE),
    ("STILL", STILL),
    ("OSCILLATES", OSCILLATES),
    ("SHIP", SHIP),
    ("GROWS", GROWS),
    ("APART", APART),
    ("UNDECIDED", UNDECIDED),
    ("PERIOD", PERIOD),
    ("CHANGES", CHANGES),
    ("WAYS[0]", WAYS[0]),
    ("WAYS[1]", WAYS[1]),
    ("WAYS[2]", WAYS[2]),
    ("WAYS[3]", WAYS[3]),
    ("WAYS[4]", WAYS[4]),
    ("WAYS[5]", WAYS[5]),
    ("WAYS[6]", WAYS[6]),
    ("WAYS[7]", WAYS[7]),
    ("PIN", PIN),
    ("KEEP", KEEP),
];

/// Registers the font.
pub(super) fn plugin(app: &mut App) {
    // Not `embedded_asset!`: that is for files next to the source, and names them after
    // where they lie.
    app.world().resource::<EmbeddedAssetRegistry>().insert_asset(
        PathBuf::from("crates/cas-ui/assets/fonts/Phosphor-Bold.ttf"),
        Path::new("cas_ui/fonts/Phosphor-Bold.ttf"),
        include_bytes!("../assets/fonts/Phosphor-Bold.ttf") as &'static [u8],
    );
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
