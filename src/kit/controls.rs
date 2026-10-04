//! The controls: buttons, checkboxes and text fields, and what scrolls and what drops down.

use bevy::{
    feathers::{
        constants::fonts,
        controls::{FeathersButton, FeathersMenuPopup, FeathersTextInputContainer},
        cursor::EntityCursor,
        palette,
        theme::{ThemeBackgroundColor, ThemeTextColor},
        tokens,
    },
    picking::hover::Hovered,
    prelude::*,
    text::{FontSourceTemplate, FontWeight, LetterSpacing},
    ui::{Checked, UiGlobalTransform},
    ui_widgets::{Checkbox, Scrollbar},
    window::{PrimaryWindow, SystemCursorIcon},
};

use super::{aspect::Aspect, icons, text::key_hint};

/// A small square button with an icon on it, as the rows of the lists have them. In a row
/// that takes clicks itself, whoever handles its click keeps the click from the row.
pub(crate) fn icon_button(glyph: &'static str, ink: Color) -> impl Scene {
    icon_button_marked(glyph, ink, Unmarked)
}

/// The mark of an icon that nobody needs to find again.
#[derive(Component, Default, Clone)]
pub(crate) struct Unmarked;

/// Such a button with a mark on its icon, for whoever changes the icon's colour later.
pub(crate) fn icon_button_marked<M: Component + Clone + Default + Unpin>(
    glyph: &'static str,
    ink: Color,
    mark: M,
) -> impl Scene {
    bsn! {
        @FeathersButton {
            @caption: bsn! { icons::icon(glyph, 14.0, ink) template_value(mark) }
        }
        Node {
            width: px(24),
            min_width: px(24),
            padding: px(0),
            justify_content: JustifyContent::Center,
            flex_shrink: 0.0,
        }
    }
}

/// The side of the box of a [`checkbox`].
const TICK_BOX: f32 = 18.0;

/// The box of a [`checkbox`], and the tick in it.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct ToggleBox;

#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct ToggleTick;

/// A checkbox ticked in the colour of an aspect, with the key that flips it, if it has one.
/// Whoever makes it keeps its `Checked` in step with what it stands for.
pub(crate) fn checkbox(label: &'static str, name: &'static str, aspect: Aspect, key: &'static str) -> impl Scene {
    let name = Name::new(name);
    bsn! {
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
        }
        Checkbox
        Hovered
        EntityCursor::System(SystemCursorIcon::Pointer)
        template_value(name)
        template_value(aspect)
        Children [
            (
                Node {
                    width: px(TICK_BOX),
                    height: px(TICK_BOX),
                    flex_shrink: 0.0,
                    margin: UiRect { right: px(8) },
                    border_radius: px(4),
                }
                BackgroundColor(palette::GRAY_3)
                ToggleBox
                Children [(
                    // The tick: two sides of a rectangle, turned by an eighth.
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(6),
                        top: px(2),
                        width: px(6),
                        height: px(11),
                        border: UiRect { bottom: px(2), right: px(2) },
                    }
                    UiTransform::from_rotation(Rot2::FRAC_PI_4)
                    Visibility::Hidden
                    ToggleTick
                )]
            ),
            (
                Text(label)
                TextFont {
                    font: FontSourceTemplate::Handle(fonts::REGULAR),
                    font_size: FontSize::Px(14.0),
                    weight: FontWeight::NORMAL,
                }
                ThemeTextColor(tokens::TEXT_MAIN)
            ),
            key_hint(key),
        ]
    }
}

/// Colours the toggles: a ticked one has the colour of its aspect.
pub(crate) fn style_toggles(
    toggles: Query<(&Aspect, &Hovered, Has<Checked>, &Children), With<Checkbox>>,
    mut boxes: Query<(&mut BackgroundColor, &Children), With<ToggleBox>>,
    mut ticks: Query<(&mut BorderColor, &mut Visibility), With<ToggleTick>>,
) {
    for (aspect, hovered, checked, children) in &toggles {
        let Some((mut fill, children)) = children.first().and_then(|&child| boxes.get_mut(child).ok()) else {
            continue;
        };
        let color = if checked { aspect.color() } else { palette::GRAY_3 };
        let color = if hovered.0 { color.lighter(0.06) } else { color };
        fill.set_if_neq(BackgroundColor(color));
        let Some((mut ink, mut visibility)) = children.first().and_then(|&child| ticks.get_mut(child).ok()) else {
            continue;
        };
        ink.set_if_neq(BorderColor::all(aspect.ink()));
        let shown = if checked { Visibility::Inherited } else { Visibility::Hidden };
        visibility.set_if_neq(shown);
    }
}

/// The frame of a text field: a well in its card, which shows while there is nothing in the
/// field, with the text a little way in from its edges.
pub(crate) fn field_frame() -> impl Scene {
    bsn! {
        @FeathersTextInputContainer
        ThemeBackgroundColor(tokens::WINDOW_BG)
        Node {
            border: UiRect::ZERO,
            padding: UiRect::horizontal(px(6)),
        }
    }
}

/// The name of a group of items in a menu.
pub(crate) fn menu_heading(text: &'static str) -> impl Scene {
    bsn! {
        Node {
            padding: UiRect { left: px(8), right: px(8), top: px(5), bottom: px(2) },
            flex_shrink: 0.0,
        }
        Children [(
            Text(text)
            TextFont {
                font: FontSourceTemplate::Handle(fonts::BOLD),
                font_size: FontSize::Px(9.0),
                weight: FontWeight::BOLD,
            }
            template_value(LetterSpacing::Px(0.5))
            ThemeTextColor(tokens::TEXT_DIM)
        )]
    }
}

/// A scrollbar is there while there is something to scroll: what it scrolls is higher than
/// the room it has.
pub(crate) fn show_scrollbars(areas: Query<&ComputedNode>, mut bars: Query<(&Scrollbar, &mut Node)>) {
    for (bar, mut node) in &mut bars {
        let Ok(area) = areas.get(bar.target) else {
            continue;
        };
        let scrolls = area.content_size().y > area.size().y + 0.5;
        let display = if scrolls { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
    }
}

/// A menu reaches down as far as the window does, and scrolls beyond that: it hangs under
/// its button, which may be anywhere in a window of any height.
pub(crate) fn fit_menus(
    window: Single<&Window, With<PrimaryWindow>>,
    menus: Query<(&ComputedNode, &UiGlobalTransform)>,
    mut popups: Query<(&mut Node, &ChildOf), With<FeathersMenuPopup>>,
) {
    /// The room between the button and its menu, and between the menu and the window's edge.
    const MARGINS: f32 = 12.0;
    for (mut popup, menu) in &mut popups {
        let Ok((node, transform)) = menus.get(menu.parent()) else {
            continue;
        };
        let bottom = (transform.translation.y + 0.5 * node.size.y) * node.inverse_scale_factor;
        let room = px((window.height() - bottom - MARGINS).max(4.0 * MARGINS));
        if popup.max_height != room {
            popup.max_height = room;
        }
    }
}
