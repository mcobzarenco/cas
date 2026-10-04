//! The controls: buttons, checkboxes, sliders, text fields and chips, and what scrolls and what
//! drops down.

use bevy::{
    feathers::{
        constants::fonts,
        controls::{FeathersButton, FeathersMenuPopup, FeathersScrollbar, FeathersTextInputContainer},
        cursor::EntityCursor,
        palette,
        theme::{ThemeBackgroundColor, ThemeTextColor, ThemedText},
        tokens,
    },
    picking::hover::Hovered,
    prelude::*,
    text::{FontSourceTemplate, FontWeight, LetterSpacing},
    ui::{Checked, UiGlobalTransform},
    ui_widgets::{
        Checkbox, ControlOrientation, ScrollArea, Scrollbar, Slider, SliderDragState, SliderOrientation, SliderThumb,
        SliderValue, TrackClick,
    },
    window::{PrimaryWindow, SystemCursorIcon},
};

use super::{KitSystems, aspect::Aspect, cards::GUTTER, icons, text::key_hint};

/// The systems that keep the controls looking as they should.
pub(super) fn plugin(app: &mut App) {
    app.add_systems(Update, (style_toggles, style_sliders, style_chips, show_scrollbars, fit_menus).in_set(KitSystems));
}

/// Brings the `Checked` of a checkbox or of a chip in line with what it stands for: `is` says
/// whether it has it, `should` whether it should.
pub(crate) fn check(commands: &mut Commands, entity: Entity, is: bool, should: bool) {
    match (should, is) {
        (true, false) => commands.entity(entity).insert(Checked),
        (false, true) => commands.entity(entity).remove::<Checked>(),
        _ => return,
    };
}

/// The caption of a button that has a key: its label and the key that does the same.
fn label_with_key(label: &'static str, key: &'static str) -> Box<dyn SceneList> {
    bsn_list![
        (Text(label) ThemedText),
        key_hint(key),
    ]
    .into()
}

/// A button with the key that does the same after its label. It takes its share of the row
/// it is in.
pub(crate) fn button(label: &'static str, key: &'static str) -> impl Scene {
    bsn! {
        @FeathersButton {
            @caption: {label_with_key(label, key)},
        }
        Node { flex_grow: 1.0 }
    }
}

/// A small square button with an icon on it, as the rows of the lists have them. A click on
/// it is its own: a row that takes clicks itself does not get it.
pub(crate) fn icon_button(glyph: &'static str, ink: Color) -> impl Scene {
    icon_button_marked(glyph, ink, Unmarked)
}

/// The mark of what nobody needs to find again.
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
struct ToggleBox;

#[derive(Component, Clone, Copy, Debug, Default)]
struct ToggleTick;

/// A checkbox ticked in the colour of an aspect, with the key that flips it, if it has one.
/// Whoever makes it keeps its `Checked` in step with what it stands for ([`check`]).
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
fn style_toggles(
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

const SLIDER_HEIGHT: f32 = 18.0;
const THUMB: f32 = 14.0;
const RAIL: f32 = 4.0;

/// The filled part of a slider's rail.
#[derive(Component, Clone, Copy, Debug, Default)]
struct SliderFill;

/// A slider with a rail, a fill and a thumb, on top of the headless `Slider` widget: a click
/// on the rail asks for the value there, a drag for the one under the pointer. Whoever makes
/// it answers its `ValueChange` with the `SliderValue` it is to have, the one asked for or one
/// near it. The fill has the colour of an aspect. The thumb travels inside a box that is one
/// thumb narrower than the slider, so plain percentages place it.
pub(crate) fn slider(aspect: Aspect) -> impl Scene {
    let fill = aspect.color();
    bsn! {
        Node {
            height: px(SLIDER_HEIGHT),
            align_items: AlignItems::Center,
        }
        Slider {
            track_click: TrackClick::Snap,
            orientation: SliderOrientation::Horizontal,
        }
        Hovered
        EntityCursor::System(SystemCursorIcon::Pointer)
        Children [
            (
                Node {
                    position_type: PositionType::Absolute,
                    left: px(0),
                    right: px(0),
                    height: px(RAIL),
                    border_radius: BorderRadius::MAX,
                }
                BackgroundColor(palette::GRAY_3)
            ),
            (
                Node {
                    position_type: PositionType::Absolute,
                    left: px(0),
                    right: px(THUMB),
                    top: px(0),
                    bottom: px(0),
                    align_items: AlignItems::Center,
                }
                Children [
                    (
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(0),
                            width: percent(0),
                            height: px(RAIL),
                            border_radius: BorderRadius::MAX,
                        }
                        BackgroundColor(fill)
                        SliderFill
                    ),
                    (
                        Node {
                            position_type: PositionType::Absolute,
                            left: percent(0),
                            width: px(THUMB),
                            height: px(THUMB),
                            border_radius: BorderRadius::MAX,
                        }
                        BackgroundColor(palette::LIGHT_GRAY_1)
                        SliderThumb
                    ),
                ]
            ),
        ]
    }
}

/// Places each slider's thumb and fill, and highlights the thumb while hovered or dragged.
fn style_sliders(
    sliders: Query<
        (Entity, &SliderValue, &Hovered, &SliderDragState),
        (With<Slider>, Or<(Changed<SliderValue>, Changed<Hovered>, Changed<SliderDragState>)>),
    >,
    children: Query<&Children>,
    mut thumbs: Query<(&mut Node, &mut BackgroundColor), (With<SliderThumb>, Without<SliderFill>)>,
    mut fills: Query<&mut Node, (With<SliderFill>, Without<SliderThumb>)>,
) {
    for (slider, value, hovered, drag) in &sliders {
        let position = percent(100.0 * value.0.clamp(0.0, 1.0));
        let color = if hovered.0 || drag.dragging { palette::WHITE } else { palette::LIGHT_GRAY_1 };
        for child in children.iter_descendants(slider) {
            if let Ok((mut node, mut background)) = thumbs.get_mut(child) {
                node.left = position;
                background.0 = color;
            }
            if let Ok(mut node) = fills.get_mut(child) {
                node.width = position;
            }
        }
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

/// What a chip is known by: an icon, an icon turned by so many degrees, or a few characters.
#[derive(Clone, Copy)]
pub(crate) enum Sign {
    Icon(&'static str),
    Turned(&'static str, f32),
    Written(&'static str),
}

/// The marks of a [`chip_box`] and of the sign of a [`chip`].
#[derive(Component, Default, Clone)]
struct ChipBox;

#[derive(Component, Default, Clone)]
struct ChipSign;

/// The box of a chip, or of a small button among chips. It lights up under the pointer, and
/// while it is `Checked` it is outlined in the colour of an aspect. Whoever makes it keeps
/// its `Checked` in step with what it stands for ([`check`]).
pub(crate) fn chip_box(aspect: Aspect) -> impl Scene {
    bsn! {
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(6),
            padding: UiRect::axes(px(7), px(3)),
            border: px(1),
            border_radius: px(4),
        }
        BackgroundColor(palette::GRAY_2)
        BorderColor::all(Color::NONE)
        Hovered
        EntityCursor::System(SystemCursorIcon::Pointer)
        ChipBox
        template_value(aspect)
    }
}

/// A chip: its box, its sign, and a word or two, or none where the sign says it all. The
/// sign is bright while the chip is on.
pub(crate) fn chip(sign: Sign, label: &'static str, aspect: Aspect) -> impl Scene {
    chip_marked(sign, label, aspect, Unmarked)
}

/// Such a chip with a mark on its sign, for whoever writes something else there later.
pub(crate) fn chip_marked<M: Component + Clone + Default + Unpin>(
    sign: Sign,
    label: &'static str,
    aspect: Aspect,
    mark: M,
) -> impl Scene {
    // A sign alone needs no word next to it, and less room around it.
    let (shown, sides) = if label.is_empty() { (Display::None, 5.0) } else { (Display::Flex, 7.0) };
    let (glyph, font, size, degrees): (_, _, f32, f32) = match sign {
        Sign::Icon(glyph) => (glyph, icons::FONT, 15.0, 0.0),
        Sign::Turned(glyph, degrees) => (glyph, icons::FONT, 15.0, degrees),
        Sign::Written(text) => (text, fonts::MONO, 13.0, 0.0),
    };
    let turned = UiTransform::from_rotation(Rot2::degrees(degrees));
    bsn! {
        chip_box(aspect)
        Node { padding: UiRect::axes(px(sides), px(3)) }
        Children [
            (
                Text(glyph)
                TextFont {
                    font: FontSourceTemplate::Handle(font),
                    font_size: FontSize::Px(size),
                    weight: FontWeight::NORMAL,
                }
                TextColor(palette::LIGHT_GRAY_2)
                ChipSign
                template_value(turned)
                template_value(mark)
                template_value(Pickable::IGNORE)
            ),
            (
                Text(label)
                TextFont {
                    font: FontSourceTemplate::Handle(fonts::REGULAR),
                    font_size: FontSize::Px(12.0),
                    weight: FontWeight::NORMAL,
                }
                TextColor(palette::LIGHT_GRAY_1)
                Node { display: shown }
                template_value(Pickable::IGNORE)
            ),
        ]
    }
}

/// Lights the chips: the box under the pointer, and the outline and the sign of one that is
/// on.
fn style_chips(
    mut chips: Query<
        (&Aspect, &Hovered, Has<Checked>, &mut BackgroundColor, &mut BorderColor, Option<&Children>),
        With<ChipBox>,
    >,
    mut signs: Query<&mut TextColor, With<ChipSign>>,
) {
    for (aspect, hovered, on, mut fill, mut border, children) in &mut chips {
        let color = if hovered.0 { palette::GRAY_3 } else { palette::GRAY_2 };
        fill.set_if_neq(BackgroundColor(color));
        let outline = if on { aspect.color() } else { Color::NONE };
        border.set_if_neq(BorderColor::all(outline));
        let ink = if on { palette::WHITE } else { palette::LIGHT_GRAY_2 };
        for &child in children.into_iter().flatten() {
            if let Ok(mut color) = signs.get_mut(child) {
                color.set_if_neq(TextColor(ink));
            }
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

/// What scrolls, and with that where its scrollbar lies and how far apart the things in it are.
#[derive(Clone, Copy)]
pub(crate) enum Scrolls {
    /// The rows of a list: the scrollbar beside them, in room that is kept for it.
    Rows,
    /// The body of a side panel: the scrollbar in the padding of the panel's card.
    Body,
    /// The cards of a column: the scrollbar in the gutter beside them.
    Cards,
}

/// A node that scrolls when what is in it is higher than the room it has, and its scrollbar,
/// which is there only then. `area` is put on the node that scrolls: the name it goes by, a
/// mark to find it by, what is in it.
pub(crate) fn scrolling(what: Scrolls, area: impl Scene) -> impl Scene {
    // The room kept beside what scrolls, the gap between the things in it, and how far out
    // and how wide the scrollbar is.
    let (room, gap, right, width) = match what {
        Scrolls::Rows => (10.0, 4.0, 0.0, 6.0),
        Scrolls::Body => (0.0, 12.0, -10.0, 6.0),
        Scrolls::Cards => (0.0, GUTTER, -6.0, 4.0),
    };
    bsn! {
        // The frame holds the scrollbar; what is in the other node scrolls.
        Node {
            flex_grow: 1.0,
            min_height: px(0),
            flex_direction: FlexDirection::Column,
            padding: UiRect { right: px(room) },
        }
        Children [
            (
                #Scrolls
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(gap),
                    overflow: Overflow::scroll_y(),
                }
                ScrollArea
                area
            ),
            (
                @FeathersScrollbar {
                    @target: #Scrolls,
                    @orientation: {ControlOrientation::Vertical}
                }
                Node {
                    display: Display::None,
                    position_type: PositionType::Absolute,
                    right: px(right),
                    top: px(0),
                    bottom: px(0),
                    width: px(width),
                }
            ),
        ]
    }
}

/// A scrollbar is there while there is something to scroll: what it scrolls is higher than
/// the room it has.
fn show_scrollbars(areas: Query<&ComputedNode>, mut bars: Query<(&Scrollbar, &mut Node)>) {
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
fn fit_menus(
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
