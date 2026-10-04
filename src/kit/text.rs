//! Text: the faces, sizes and shades it comes in, and how numbers are written.

use bevy::{
    feathers::{constants::fonts, theme::ThemeTextColor, tokens},
    prelude::*,
    text::{FontSourceTemplate, FontWeight},
};

/// Small dim explanatory text.
pub(crate) fn caption(text: impl Into<String>) -> impl Scene {
    bsn! {
        Text(text)
        TextFont {
            font: FontSourceTemplate::Handle(fonts::REGULAR),
            font_size: FontSize::Px(12.0),
            weight: FontWeight::NORMAL,
        }
        ThemeTextColor(tokens::TEXT_DIM)
    }
}

/// Monospace readout text.
pub(crate) fn readout(text: impl Into<String>) -> impl Scene {
    bsn! {
        Text(text)
        TextFont {
            font: FontSourceTemplate::Handle(fonts::MONO),
            font_size: FontSize::Px(12.0),
            weight: FontWeight::NORMAL,
        }
        ThemeTextColor(tokens::TEXT_MAIN)
    }
}

/// A column title of a list; the name of a field or of a group as well.
pub(crate) fn heading(title: &'static str) -> impl Scene {
    bsn! {
        Text(title)
        TextFont {
            font: FontSourceTemplate::Handle(fonts::BOLD),
            font_size: FontSize::Px(10.0),
            weight: FontWeight::BOLD,
        }
        ThemeTextColor(tokens::TEXT_DIM)
    }
}

/// Text in the fixed-width face, as figures and speeds are set. Clicks go through it.
pub(crate) fn mono(text: String, size: f32, color: Color) -> impl Scene {
    bsn! {
        Text(text)
        TextFont {
            font: FontSourceTemplate::Handle(fonts::MONO),
            font_size: FontSize::Px(size),
            weight: FontWeight::NORMAL,
        }
        TextColor(color)
        template_value(Pickable::IGNORE)
    }
}

/// The size at which so many letters of the fixed-width face fit in so much room, and no
/// larger than `most`. The period of a slow spaceship has seven figures, and its speed more:
/// they are set smaller rather than cut short.
pub(crate) fn fitting(letters: usize, room: f32, most: f32) -> f32 {
    // A letter of that face is six tenths of its size wide.
    (room / (0.6 * letters.max(1) as f32)).clamp(8.0, most)
}

/// A figure set to the right of its column.
pub(crate) fn number(text: String, column: f32, color: Color) -> impl Scene {
    let size = fitting(text.chars().count(), column, 12.0);
    bsn! {
        mono(text, size, color)
        TextLayout { justify: Justify::Right }
        Node { width: px(column) }
    }
}

/// Shortcut reminders are legible on a button of any colour, and quiet.
const KEY_HINT: Color = Color::srgba(1.0, 1.0, 1.0, 0.45);

/// A quiet reminder of a shortcut, placed right after the label of its control.
pub(crate) fn key_hint(keys: impl Into<String>) -> impl Scene {
    bsn! {
        Text(keys)
        TextFont {
            font: FontSourceTemplate::Handle(fonts::MONO),
            font_size: FontSize::Px(10.0),
            weight: FontWeight::NORMAL,
        }
        TextColor(KEY_HINT)
        Node {
            margin: UiRect { left: px(6) },
        }
    }
}

/// `1234567` → `1 234 567`, for a number of any width.
pub(crate) fn group_digits(n: impl TryInto<i128>) -> String {
    let n = n.try_into().unwrap_or(i128::MAX);
    let digits = n.unsigned_abs().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    if n < 0 {
        out.push('-');
    }
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digits_are_grouped_in_threes() {
        assert_eq!(group_digits(-1234567), "-1 234 567");
        assert_eq!(group_digits(69_481_732_320_u128), "69 481 732 320");
    }
}
