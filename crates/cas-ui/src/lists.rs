//! The rows of lists, and what the lists of patterns share: their columns, the picture of a
//! pattern, and the dial of the ways it flies.

use bevy::{
    feathers::{cursor::EntityCursor, palette},
    picking::hover::Hovered,
    prelude::*,
    window::SystemCursorIcon,
};

use super::{
    KitSystems,
    aspect::{ALIVE, Aspect, BLOCKS, DEAD},
    controls::Unmarked,
    icons,
};

/// The system that keeps the rows looking as they should.
pub(super) fn plugin(app: &mut App) {
    app.add_systems(Update, light_rows.in_set(KitSystems));
}

/// The mark of a [`list_row`].
#[derive(Component, Default, Clone)]
struct Row;

/// A row of a list that a click does something with: its box, which lights up under the
/// pointer. How its contents lie in it is the caller's, and so is its outline: of the row
/// that is chosen, say.
pub fn list_row() -> impl Scene {
    bsn! {
        Node {
            padding: UiRect::axes(px(7), px(5)),
            border: px(1),
            border_radius: px(5),
            flex_shrink: 0.0,
        }
        BackgroundColor(palette::GRAY_2)
        BorderColor::all(Color::NONE)
        Hovered
        EntityCursor::System(SystemCursorIcon::Pointer)
        Row
    }
}

/// A row lights up under the pointer, since a click on it does something.
fn light_rows(mut rows: Query<(&Hovered, &mut BackgroundColor), With<Row>>) {
    for (hovered, mut background) in &mut rows {
        let color = if hovered.0 { palette::GRAY_3 } else { palette::GRAY_2 };
        if background.0 != color {
            background.0 = color;
        }
    }
}

/// A thin bar under what a row says: so much of it, from 0 to 1, is filled in the colour of
/// an aspect.
pub fn share_bar(share: f32, aspect: Aspect) -> impl Scene {
    share_bar_marked(share, aspect, Unmarked)
}

/// Such a bar with a mark on the part that is filled, for whoever changes its width later.
pub fn share_bar_marked<M: Component + Clone + Default + Unpin>(share: f32, aspect: Aspect, mark: M) -> impl Scene {
    let fill = aspect.color();
    bsn! {
        Node {
            height: px(3),
            border_radius: BorderRadius::MAX,
        }
        BackgroundColor(palette::GRAY_0)
        template_value(Pickable::IGNORE)
        Children [(
            Node {
                width: percent(100.0 * share),
                height: percent(100),
                border_radius: BorderRadius::MAX,
            }
            BackgroundColor(fill)
            template_value(Pickable::IGNORE)
            template_value(mark)
        )]
    }
}

/// Column widths of the lists of patterns, shared by their headers and their rows. What they
/// leave must be room enough for the likes of `2c/184 ↘`, or a row would widen its panel.
pub const PICTURE: (f32, f32) = (64.0, 44.0);
pub const PERIOD_COLUMN: f32 = 40.0;
pub const CELLS_COLUMN: f32 = 30.0;
pub const COLUMN_GAP: f32 = 6.0;

/// The side of the dial that shows which ways the ships of a kind fly.
const DIAL: f32 = 30.0;
/// How bright a way of the dial is that the fewest ships took, against the one most took.
const FAINTEST: f32 = 0.45;
/// A way of the dial that a ship of the kind could go and none did, and one that is not for
/// the kind at all: lighter and darker than a row, lit by the pointer or not.
const POSSIBLE: Color = Color::oklcha(0.54, 0.006, 282.0, 1.0);
const IMPOSSIBLE: Color = palette::GRAY_1;

/// The eight ways there are to go on a grid, clockwise from straight up: a step across and a
/// step down each.
const WAYS: [(i32, i32); 8] = [(0, -1), (1, -1), (1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1)];

/// An arrow of a dial: the way it points, of the eight there are, clockwise from straight up;
/// whether the kind can go that way; and which kind of the list it belongs to, if it is the
/// list's: those are kept in step with what is caught ([`glow`]).
#[derive(Component, Default, Clone, Copy)]
pub struct Flown {
    pub kind: Option<usize>,
    pub way: usize,
    pub possible: bool,
}

/// The ways the spaceships of a kind go, as a dial of the eight ways there are. A way that
/// some went is lit, and the brighter the more of them did; a way that none went and one
/// could is grey; a way that is not for the kind at all is dark. `ways` counts the ships by
/// the way they went, clockwise from straight up, and `possible` says of each way whether the
/// kind can go it, which is for whoever knows the rule to say. `kind` is the kind's place in
/// the spaceship list, whose dials follow what is caught. With no way to show there is no
/// dial.
///
/// Every way has an arrow of its own, where one arrow could be turned eight ways: what is
/// turned is not clipped by what it scrolls in, and a dial is in every row of lists that do.
pub fn dial(ways: &[u64; 8], possible: &[bool; 8], kind: Option<usize>) -> impl Scene + use<> {
    let step = DIAL / 3.0;
    let arrows: Vec<_> = WAYS
        .iter()
        .enumerate()
        .map(|(way, &(dx, dy))| {
            let color = glow(ways, way, possible[way]);
            let flown = Flown { kind, way, possible: possible[way] };
            bsn! {
                Node {
                    position_type: PositionType::Absolute,
                    left: px(step * (1 + dx) as f32),
                    top: px(step * (1 + dy) as f32),
                    width: px(step),
                    height: px(step),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                }
                template_value(Pickable::IGNORE)
                Children [(
                    icons::icon(icons::WAYS[way], step, color)
                    template_value(flown)
                    template_value(Pickable::IGNORE)
                )]
            }
        })
        .collect();
    let none = ways.iter().all(|&ships| ships == 0) && possible.iter().all(|&possible| !possible);
    let (side, shown) = if none { (0.0, Display::None) } else { (DIAL, Display::Flex) };
    bsn! {
        Node {
            display: shown,
            width: px(side),
            height: px(side),
            flex_shrink: 0.0,
        }
        template_value(Pickable::IGNORE)
        Children [ { arrows } ]
    }
}

/// The colour of a way on a dial. Where no ship went: dark, or grey if one could have. Where
/// some did: the brighter the more of them, up to the way most of them took.
pub fn glow(ways: &[u64; 8], way: usize, possible: bool) -> Color {
    let most = ways.iter().copied().max().unwrap_or(0);
    if ways[way] == 0 {
        return if possible { POSSIBLE } else { IMPOSSIBLE };
    }
    let share = ways[way] as f32 / most as f32;
    let (dark, lit) = (palette::GRAY_3.to_srgba(), Aspect::Pattern.color().to_srgba());
    dark.mix(&lit, FAINTEST + (1.0 - FAINTEST) * share).into()
}

/// The pattern drawn small: one square per cell, as large as fits the box, on the blocks it
/// lies on. The cells are given relative to a corner of the blocks the next step rewrites, so
/// the picture is cut at block boundaries and shows them, as the grid does: how a pattern sits
/// on the blocks is part of what it is.
pub fn picture(cells: &[(i32, i32)]) -> impl Scene {
    // Whole blocks: the bounding box widened to even coordinates on the left and the top (a
    // settled pattern starts at 0 or 1 either way) and to odd ones on the right and the bottom.
    let span = |axis: fn(&(i32, i32)) -> i32| {
        let (min, max) = cells.iter().map(axis).fold((0, 0), |(lo, hi), v| (lo.min(v), hi.max(v)));
        (min & !1, max | 1)
    };
    let ((left, right), (top, bottom)) = (span(|cell| cell.0), span(|cell| cell.1));
    let (width, height) = ((right - left + 1) as f32, (bottom - top + 1) as f32);
    let side = ((PICTURE.0 - 12.0) / width).min((PICTURE.1 - 12.0) / height).floor().clamp(1.0, 7.0);
    // A hairline between neighbours, once the squares are big enough to spare it.
    let ink = if side >= 4.0 { side - 1.0 } else { side };
    let squares: Vec<_> = cells
        .iter()
        .map(|&(x, y)| {
            let (x, y) = ((x - left) as f32 * side, (y - top) as f32 * side);
            bsn! {
                Node {
                    position_type: PositionType::Absolute,
                    left: px(x),
                    top: px(y),
                    width: px(ink),
                    height: px(ink),
                }
                BackgroundColor(ALIVE)
            }
        })
        .collect();
    // The block boundaries, in the hairlines between the squares.
    let line_color = BLOCKS.0.with_alpha(BLOCKS.1);
    let lines: Vec<_> = (0..=(width as i32) / 2)
        .map(|k| (true, k))
        .chain((0..=(height as i32) / 2).map(|k| (false, k)))
        .map(|(vertical, k)| {
            let at = 2.0 * k as f32 * side - 1.0;
            let (left, top) = if vertical { (at, -1.0) } else { (-1.0, at) };
            let (w, h) = if vertical { (1.0, height * side + 1.0) } else { (width * side + 1.0, 1.0) };
            bsn! {
                Node {
                    position_type: PositionType::Absolute,
                    left: px(left),
                    top: px(top),
                    width: px(w),
                    height: px(h),
                }
                BackgroundColor(line_color)
            }
        })
        .collect();
    bsn! {
        Node {
            width: px(PICTURE.0),
            height: px(PICTURE.1),
            flex_shrink: 0.0,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border_radius: px(4),
        }
        BackgroundColor(DEAD)
        template_value(Pickable::IGNORE)
        Children [(
            Node {
                width: px(width * side),
                height: px(height * side),
            }
            Children [ { squares }, { lines } ]
        )]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_way_of_the_dial_is_dark_grey_or_lit() {
        // No ship went up: dark if none can, grey if one could.
        let ways = [0, 0, 6, 0, 3, 0, 0, 0];
        assert_eq!(glow(&ways, 0, false), IMPOSSIBLE);
        assert_eq!(glow(&ways, 0, true), POSSIBLE);
        // Where ships went, the way most of them took is the colour of patterns, and the
        // other is between that and the dark: whether it is possible is not asked again.
        assert_eq!(glow(&ways, 2, true), Color::from(Aspect::Pattern.color().to_srgba()));
        let (fewer, most) = (glow(&ways, 4, true).to_srgba(), glow(&ways, 2, true).to_srgba());
        assert!(fewer.red < most.red && fewer.red > palette::GRAY_3.to_srgba().red);
        assert_eq!(glow(&ways, 4, false), glow(&ways, 4, true));
    }
}
