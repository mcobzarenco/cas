//! What things are put in: cards, side panels, sections and tiles.

use bevy::{
    feathers::{constants::fonts, palette, theme::ThemeTextColor, tokens},
    prelude::*,
    text::{FontSourceTemplate, FontWeight, LetterSpacing},
};

use super::{aspect::Aspect, controls::button};

/// What cards are made of. They lie on the window's background, a shade darker than they are.
pub(crate) const CARD: Color = palette::GRAY_1;
/// The room between cards, and around them.
pub(crate) const GUTTER: f32 = 8.0;

/// A card: the controls of one aspect under its name. `figure` is what the card has to show
/// of its own, if anything; it goes next to the name.
pub(crate) fn card(aspect: Aspect, figure: impl SceneList, body: impl SceneList) -> impl Scene {
    bsn! {
        Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            row_gap: px(6),
            padding: UiRect::axes(px(12), px(10)),
            border_radius: px(8),
        }
        BackgroundColor(CARD)
        Children [
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::SpaceBetween,
                    margin: UiRect { bottom: px(2) },
                }
                Children [
                    title(aspect, aspect.title()),
                    { figure },
                ]
            ),
            { body },
        ]
    }
}

/// The name of a card with the mark of its aspect in front. The mark carries the colour; the
/// text stays text-coloured and legible.
fn title(aspect: Aspect, text: &'static str) -> impl Scene {
    bsn! {
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(7),
        }
        Children [
            mark(aspect, 12.0),
            (
                Text(text)
                TextFont {
                    font: FontSourceTemplate::Handle(fonts::BOLD),
                    font_size: FontSize::Px(11.0),
                    weight: FontWeight::BOLD,
                }
                template_value(LetterSpacing::Px(0.6))
                ThemeTextColor(tokens::TEXT_MAIN)
            ),
        ]
    }
}

/// A short bar in the colour of an aspect.
fn mark(aspect: Aspect, height: f32) -> impl Scene {
    let color = aspect.color();
    bsn! {
        Node {
            width: px(4),
            height: px(height),
            border_radius: px(2),
        }
        BackgroundColor(color)
    }
}

/// A side panel: one tall card beside the control panel, hidden until it is asked for.
pub(crate) fn side_panel(width: f32, body: impl SceneList) -> impl Scene {
    bsn! {
        Node {
            display: Display::None,
            width: px(width),
            height: percent(100),
            flex_shrink: 0.0,
            padding: UiRect { top: px(GUTTER), bottom: px(GUTTER), right: px(GUTTER) },
        }
        Children [(
            // A card is as wide as its panel, whatever is in it: content that does not fit
            // overflows the card, rather than the card the panel.
            Node {
                flex_grow: 1.0,
                flex_basis: px(0),
                min_width: px(0),
                flex_direction: FlexDirection::Column,
                padding: px(14),
                row_gap: px(12),
                border_radius: px(8),
            }
            BackgroundColor(CARD)
            Children [ { body } ]
        )]
    }
}

/// The name of a side panel, marked with the aspect the panel is about.
pub(crate) fn panel_title(aspect: Aspect, text: &'static str) -> impl Scene {
    bsn! {
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(8),
        }
        Children [
            mark(aspect, 16.0),
            (
                Text(text)
                TextFont {
                    font: FontSourceTemplate::Handle(fonts::BOLD),
                    font_size: FontSize::Px(16.0),
                    weight: FontWeight::BOLD,
                }
                TextColor(palette::WHITE)
            ),
        ]
    }
}

/// The head of a side panel: its title, and across from it the button that puts the panel
/// away. `close` is put on that button: the name it goes by, and what a press does.
pub(crate) fn panel_header(title: impl Scene, close: impl Scene) -> impl Scene {
    bsn! {
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
        }
        Children [
            title,
            (button("Close") close),
        ]
    }
}

/// A titled group of controls.
pub(crate) fn section(title: &'static str, body: impl SceneList) -> impl Scene {
    bsn! {
        Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            row_gap: px(6),
        }
        Children [
            (
                Text(title)
                TextFont {
                    font: FontSourceTemplate::Handle(fonts::BOLD),
                    font_size: FontSize::Px(11.0),
                    weight: FontWeight::BOLD,
                }
                ThemeTextColor(tokens::TEXT_DIM)
            ),
            { body },
        ]
    }
}

/// The side of the box a tile has its picture in.
pub(crate) const GLYPH: f32 = 48.0;

/// A symmetry as a picture in such a box. Where a point just right of the top of a square
/// ends up under each way of turning and mirroring it, as `(x, y)` from the middle: first as
/// it is, then in the order of `TURNS_AND_MIRRORS`. The ones a rule or a pattern is itself
/// under are a picture of its symmetry.
pub(crate) const ORBIT: [(f32, f32); 8] =
    [(6.0, -16.0), (16.0, 6.0), (-6.0, 16.0), (-16.0, -6.0), (-6.0, -16.0), (6.0, 16.0), (-16.0, 6.0), (16.0, -6.0)];
/// The axes of the mirrors among those, left to right, top to bottom, and the two diagonals:
/// which of the eight each is, and how far a bar through the middle is turned to lie along it.
pub(crate) const AXES: [(usize, f32); 4] = [(4, 90.0), (5, 0.0), (6, 45.0), (7, -45.0)];

/// The box a finding is shown in: of a rule in the editor, of a pattern in the analysis.
pub(crate) fn tile() -> impl Scene {
    bsn! {
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(8),
            padding: px(6),
            border_radius: px(5),
        }
        BackgroundColor(palette::GRAY_2)
    }
}

/// The name of a finding.
pub(crate) fn tile_label(name: &'static str) -> impl Scene {
    bsn! {
        Text(name)
        TextFont {
            font: FontSourceTemplate::Handle(fonts::BOLD),
            font_size: FontSize::Px(9.0),
            weight: FontWeight::BOLD,
        }
        template_value(LetterSpacing::Px(0.5))
        TextColor(palette::LIGHT_GRAY_2)
    }
}

/// What was found, in words.
pub(crate) fn tile_value(text: &'static str) -> impl Scene {
    bsn! {
        Text(text)
        TextFont {
            font: FontSourceTemplate::Handle(fonts::REGULAR),
            font_size: FontSize::Px(12.0),
            weight: FontWeight::NORMAL,
        }
        TextColor(palette::LIGHT_GRAY_1)
    }
}
