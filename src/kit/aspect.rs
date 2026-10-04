//! The aspects of what is shown, and the colours: of the cells, of each aspect, of the theme.

use bevy::{
    feathers::{dark_theme::create_dark_theme, palette, theme::ThemeProps, tokens},
    prelude::*,
};

/// The colours of a cell that is alive and of one that is dead.
pub const ALIVE: Color = Color::srgb(1.0, 0.769, 0.42);
pub const DEAD: Color = Color::srgb(0.055, 0.059, 0.078);

/// What a control is about. Every aspect has its card in the panel and its colour.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Aspect {
    /// The law: the table that rewrites the blocks.
    #[default]
    Rule,
    /// The space the cells live in: its size, and what its edge does.
    World,
    Time,
    /// How the grid is drawn. The automaton knows nothing of it, hence no colour of its own.
    View,
    /// What is on the grid: the cells.
    Pattern,
}

impl Aspect {
    pub(crate) fn title(self) -> &'static str {
        match self {
            Aspect::Rule => "RULE",
            Aspect::World => "WORLD",
            Aspect::Time => "TIME",
            Aspect::View => "VIEW",
            Aspect::Pattern => "PATTERN",
        }
    }

    /// Lightness, chroma and hue. The hues are far apart and the lightnesses staggered, which
    /// keeps the four colours distinct to colour-blind eyes as well; the pattern's is the
    /// colour of the cells themselves.
    pub const fn color(self) -> Color {
        match self {
            Aspect::Rule => Color::oklch(0.62, 0.17, 355.0),
            Aspect::World => Color::oklch(0.72, 0.12, 195.0),
            Aspect::Time => Color::oklch(0.58, 0.16, 257.0),
            Aspect::View => Color::oklch(0.68, 0.015, 265.0),
            Aspect::Pattern => ALIVE,
        }
    }

    /// What to draw in on top of the aspect's colour: dark on the light ones.
    pub(crate) fn ink(self) -> Color {
        match self {
            Aspect::World | Aspect::View | Aspect::Pattern => DEAD,
            Aspect::Rule | Aspect::Time => palette::WHITE,
        }
    }
}

/// The feathers dark theme, with its one accent colour handed out by aspect: the play button
/// is about time, the text field about the rule, and what belongs to no aspect is grey.
pub(crate) fn theme() -> ThemeProps {
    let mut theme = create_dark_theme();
    let (time, rule) = (Aspect::Time.color(), Aspect::Rule.color());
    theme.color.extend([
        (tokens::BUTTON_PRIMARY_BG, time),
        (tokens::BUTTON_PRIMARY_BG_HOVER, time.lighter(0.05)),
        (tokens::BUTTON_PRIMARY_BG_PRESSED, time.lighter(0.1)),
        (tokens::TEXT_INPUT_CURSOR, rule.lighter(0.2)),
        (tokens::TEXT_INPUT_SELECTION, rule),
        (tokens::FOCUS_RING, palette::LIGHT_GRAY_2.with_alpha(0.5)),
        (tokens::SCROLLBAR_THUMB, palette::LIGHT_GRAY_2),
        (tokens::SCROLLBAR_THUMB_HOVER, palette::LIGHT_GRAY_1),
    ]);
    theme
}
