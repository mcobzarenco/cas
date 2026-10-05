//! The kit on one page: every element of `cas-ui`, put together the way the panels of cas put
//! them together, and none of cas in it.
//!
//! ```sh
//! cargo run --release -p cas-ui --example gallery
//! ```
//!
//! Given a path, it saves a picture of itself there and leaves:
//!
//! ```sh
//! cargo run --release -p cas-ui --example gallery -- gallery.png
//! ```

use std::time::{Duration, Instant};

use bevy::{
    feathers::{
        controls::{FeathersMenu, FeathersMenuButton, FeathersMenuItem, FeathersMenuPopup, FeathersTextInput},
        palette,
        theme::{ThemeBackgroundColor, ThemedText},
        tokens,
    },
    prelude::*,
    render::view::screenshot::{Screenshot, save_to_disk},
    ui::Checked,
    ui_widgets::{Activate, ScrollArea, SliderValue, ValueChange},
    window::{PresentMode, WindowResolution},
};

use cas_ui::{
    AXES, Aspect, CELLS_COLUMN, COLUMN_GAP, GLYPH, GUTTER, KitPlugin, KitSystems, ORBIT, PERIOD_COLUMN, PICTURE,
    Scrolls, Sign, button, caption, card, check, checkbox, chip, chip_box, dial, field_frame, group_digits, heading,
    icon_button, icons, key_hint, keyed_button, list_row, menu_heading, mono, number, panel_header, panel_title,
    picture, readout, sans, scrolling, section, section_title, share_bar, side_panel, slider, tile, tile_label,
    tile_picture, tile_value,
};

/// What the gallery keeps of its own: whether its side panel is there, and which row of its
/// list is chosen.
#[derive(Resource)]
struct Gallery {
    panel: bool,
    chosen: usize,
}

/// The side panel that the button in the first card puts away and brings back.
#[derive(Component, Default, Clone)]
struct Panel;

/// A row of the list: which.
#[derive(Component, Default, Clone, Copy)]
struct Row(usize);

/// The slider whose value the card says next to its name, and the text that says it.
#[derive(Component, Default, Clone)]
struct Measured;

#[derive(Component, Default, Clone)]
struct Measure;

/// The picture to take, and the frames gone by.
#[derive(Resource)]
struct Shot {
    path: String,
    frames: u32,
}

fn main() {
    let shot = std::env::args().nth(1);
    let mut app = App::new();
    app.add_plugins((
        DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "cas-ui: the kit".into(),
                resolution: WindowResolution::new(1240, 900),
                // Under XWayland waiting for vsync stalls the frame loop: `pace` waits
                // instead.
                present_mode: PresentMode::AutoNoVsync,
                ..default()
            }),
            ..default()
        }),
        KitPlugin,
    ))
    .insert_resource(Gallery { panel: true, chosen: 0 })
    .add_systems(Startup, spawn)
    // The kit brings its elements in line after the gallery has said what they stand for.
    .add_systems(Update, (show_panel, outline_chosen, measure).before(KitSystems))
    .add_systems(Last, pace)
    .add_observer(tick)
    .add_observer(slide);
    if let Some(path) = shot {
        // The picture is saved elsewhere, where a folder that is not there is only logged.
        if let Some(folder) = std::path::Path::new(&path).parent() {
            std::fs::create_dir_all(folder).unwrap_or_else(|e| panic!("cannot create {}: {e}", folder.display()));
        }
        app.insert_resource(Shot { path, frames: 0 }).add_systems(Update, take_picture);
    }
    app.run();
}

fn spawn(mut commands: Commands) {
    commands.spawn(Camera2d);
    commands.spawn_scene(bsn! {
        Node {
            width: percent(100),
            height: percent(100),
            flex_direction: FlexDirection::Row,
        }
        ThemeBackgroundColor(tokens::WINDOW_BG)
        Children [ cards(), list_panel(), icons_panel() ]
    });
}

/// A column of cards, one to an aspect, which scrolls in a window too low for them.
fn cards() -> impl Scene {
    bsn! {
        Node {
            width: px(300),
            height: percent(100),
            flex_direction: FlexDirection::Column,
            flex_shrink: 0.0,
            padding: px(GUTTER),
        }
        Children [
            scrolling(Scrolls::Cards, bsn! {
                Children [ buttons(), checkboxes(), sliders(), texts(), chips() ]
            }),
        ]
    }
}

/// Buttons, and a menu.
fn buttons() -> impl Scene {
    card(
        Aspect::Rule,
        bsn_list![],
        bsn_list![
            caption("A button has a word on it, or the key that does the same as well, or an icon."),
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(6),
                }
                Children [
                    (button("Plain") Node { flex_grow: 1.0 }),
                    (keyed_button("Keyed", "k") Node { flex_grow: 1.0 }),
                    icon_button(icons::LOOK, palette::LIGHT_GRAY_1),
                    icon_button(icons::KEEP, Aspect::Pattern.color()),
                ]
            ),
            (
                @FeathersMenu
                Children [
                    (
                        @FeathersMenuButton {
                            @caption: bsn! { Text("A menu") ThemedText }
                        }
                        Node { flex_grow: 1.0 }
                    ),
                    (
                        @FeathersMenuPopup
                        Node { overflow: Overflow::scroll_y(), min_width: percent(100) }
                        ScrollArea
                        Children [
                            menu_heading("PINNED"),
                            menu_item("Single rotation"),
                            menu_item("Critters"),
                            menu_heading("OF LATE"),
                            menu_item("Tron"),
                        ]
                    ),
                ]
            ),
            (
                button("Side panel")
                on(|_: On<Activate>, mut gallery: ResMut<Gallery>| gallery.panel = !gallery.panel)
            ),
        ],
    )
}

fn menu_item(label: &'static str) -> impl Scene {
    bsn! {
        @FeathersMenuItem {
            @caption: bsn! { Text(label) ThemedText }
        }
        Node { flex_shrink: 0.0 }
    }
}

/// Checkboxes, in the colour of every aspect.
fn checkboxes() -> impl Scene {
    card(
        Aspect::World,
        bsn_list![],
        bsn_list![
            caption("A checkbox is ticked in the colour of its aspect."),
            checkbox("In canonical form", "Canonical", Aspect::Rule, ""),
            (checkbox("Open border", "OpenBorder", Aspect::World, "o") Checked),
            (checkbox("Run backwards in time", "Reverse", Aspect::Time, "r") Checked),
            checkbox("Cell grid", "ShowGrid", Aspect::View, "g"),
            (checkbox("Catch spaceships", "Catching", Aspect::Pattern, "k") Checked),
        ],
    )
}

/// Sliders; the card says the value of the first next to its name.
fn sliders() -> impl Scene {
    card(
        Aspect::Time,
        bsn_list![(readout("") Measure)],
        bsn_list![
            caption("A slider is filled in the colour of its aspect."),
            (slider(Aspect::Time) SliderValue(0.3) Measured),
            (slider(Aspect::Pattern) SliderValue(0.7)),
        ],
    )
}

/// Text, in the faces, sizes and shades it comes in.
fn texts() -> impl Scene {
    card(
        Aspect::View,
        bsn_list![],
        bsn_list![
            caption("A caption: small, dim, explanatory."),
            readout(format!("A readout: {} cells", group_digits(1_234_567))),
            heading("A HEADING OF A COLUMN"),
            section_title("THE TITLE OF A SECTION"),
            tile_label("THE LABEL OF A TILE"),
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Baseline,
                    column_gap: px(8),
                }
                Children [
                    sans("Either face", 14.0, palette::WHITE),
                    mono("at any size", 14.0, palette::WHITE),
                    mono("2c/184 ↘", 11.0, palette::LIGHT_GRAY_2),
                ]
            ),
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                }
                Children [
                    caption("The key that does the same"),
                    key_hint("space"),
                ]
            ),
        ],
    )
}

/// Chips, and a text field.
fn chips() -> impl Scene {
    card(
        Aspect::Pattern,
        bsn_list![],
        bsn_list![
            caption("A chip is on or off."),
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    align_items: AlignItems::Center,
                    column_gap: px(5),
                    row_gap: px(5),
                }
                Children [
                    (chip(Sign::Written("90°"), "", Aspect::Rule) on(flip)),
                    (chip(Sign::Icon(icons::MIRROR), "", Aspect::Rule) Checked on(flip)),
                    (chip(Sign::Turned(icons::MIRROR, 45.0), "", Aspect::Rule) on(flip)),
                    (chip(Sign::Icon(icons::CELLS), "cells", Aspect::Rule) Checked on(flip)),
                    (chip(Sign::Icon(icons::WEIGHT), "a weight", Aspect::Time) Checked on(flip)),
                    (
                        chip_box(Aspect::Rule)
                        Children [(
                            icons::icon(icons::MORE, 12.0, palette::LIGHT_GRAY_1)
                            template_value(Pickable::IGNORE)
                        )]
                    ),
                ]
            ),
            (
                field_frame()
                Children [ (@FeathersTextInput {}) ]
            ),
            caption("A text field in its frame."),
        ],
    )
}

/// A side panel: tiles, and a list that scrolls, whose rows light up and take a click.
fn list_panel() -> impl Scene {
    let rows: Vec<_> = (0..PATTERNS.len()).map(row).collect();
    bsn! {
        side_panel(396.0, bsn_list![
            panel_header(panel_title(Aspect::Pattern, "A side panel"), bsn! {
                on(|_: On<Activate>, mut gallery: ResMut<Gallery>| gallery.panel = false)
            }),
            caption("A side panel is hidden until it is asked for. The button in the first card brings this one back."),
            section("TILES", bsn_list![(
                Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(6),
                }
                Children [
                    finding("SYMMETRY", "a square's", symmetry()),
                    finding("WHAT", "Spaceship", bsn_list![icons::icon(icons::SHIP, 26.0, Aspect::Pattern.color())]),
                ]
            )]),
            (
                // Titles over the columns of the rows below: same widths, same padding.
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(COLUMN_GAP),
                    padding: UiRect { left: px(8), right: px(18) },
                }
                Children [
                    (Node { width: px(PICTURE.0) } Children [ heading("PATTERN") ]),
                    (Node { flex_grow: 1.0, flex_basis: px(0) } Children [ heading("SPEED") ]),
                    (Node { width: px(PERIOD_COLUMN), justify_content: JustifyContent::End } Children [ heading("PERIOD") ]),
                    (Node { width: px(CELLS_COLUMN), justify_content: JustifyContent::End } Children [ heading("CELLS") ]),
                    (Node { width: px(24) }),
                ]
            ),
            scrolling(Scrolls::Rows, bsn! { Children [ {rows} ] }),
        ])
        Panel
    }
}

/// A finding in its tile: a picture, a label and what was found.
fn finding(label: &'static str, value: &'static str, picture: impl SceneList) -> impl Scene {
    bsn! {
        tile()
        Node {
            flex_grow: 1.0,
            flex_basis: px(0),
        }
        Children [
            (tile_picture(GLYPH) Children [ { picture } ]),
            (
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(2),
                }
                Children [ tile_label(label), tile_value(value) ]
            ),
        ]
    }
}

/// The symmetry of a square as a picture: a point and its seven images, and the four mirrors.
fn symmetry() -> impl SceneList {
    let middle = GLYPH / 2.0;
    let color = Aspect::Pattern.color();
    let axes: Vec<_> = AXES
        .into_iter()
        .map(|(_, degrees): (usize, f32)| {
            let turned = UiTransform::from_rotation(Rot2::degrees(degrees));
            bsn! {
                Node {
                    position_type: PositionType::Absolute,
                    left: px(2),
                    top: px(middle - 0.5),
                    width: px(GLYPH - 4.0),
                    height: px(1),
                }
                BackgroundColor(color)
                template_value(turned)
            }
        })
        .collect();
    let dots: Vec<_> = ORBIT
        .into_iter()
        .map(|(x, y): (f32, f32)| {
            bsn! {
                Node {
                    position_type: PositionType::Absolute,
                    left: px(middle + x - 2.5),
                    top: px(middle + y - 2.5),
                    width: px(5),
                    height: px(5),
                    border_radius: BorderRadius::MAX,
                }
                BackgroundColor(palette::WHITE)
            }
        })
        .collect();
    bsn_list![{ axes }, { dots }]
}

/// What the rows of the list show: the cells of a pattern, how fast it goes and which way,
/// how many went each of the eight ways, its period, and its share of them all.
type Pattern = (&'static [(i32, i32)], &'static str, &'static str, [u64; 8], u32, f32);

const PATTERNS: [Pattern; 12] = [
    (&[(0, 1), (1, 0), (1, 1), (2, 1)], "c/6", "orthogonal", [9, 0, 7, 0, 8, 0, 6, 0], 12, 0.42),
    (&[(0, 0), (1, 1), (2, 1), (3, 0)], "c/4", "diagonal", [0, 5, 0, 3, 0, 4, 0, 6], 8, 0.21),
    (&[(0, 0), (0, 1), (1, 2), (2, 0), (2, 1), (3, 3)], "2c/184", "orthogonal", [1, 0, 0, 0, 2, 0, 0, 0], 368, 0.12),
    (&[(1, 0), (0, 1), (2, 2), (3, 1)], "c/15", "oblique", [0, 0, 0, 1, 0, 0, 0, 0], 30, 0.08),
    (&[(0, 0), (1, 0), (0, 1), (1, 1)], "2×2", "still", [0; 8], 1, 0.06),
    (&[(0, 0), (3, 0), (1, 1), (2, 1), (0, 3), (3, 3)], "c/2", "orthogonal", [2, 0, 2, 0, 2, 0, 2, 0], 4, 0.04),
    (&[(1, 0), (0, 1), (1, 2), (2, 1), (5, 1)], "3c/7", "diagonal", [0, 1, 0, 1, 0, 0, 0, 0], 14, 0.03),
    (&[(0, 0), (2, 0), (4, 0), (1, 1), (3, 1)], "c/40", "orthogonal", [0, 0, 3, 0, 0, 0, 1, 0], 80, 0.02),
    (&[(0, 0), (1, 1), (2, 2), (3, 3), (4, 4), (5, 5)], "c", "diagonal", [0, 0, 0, 9, 0, 0, 0, 0], 2, 0.02),
    (
        &[(0, 0), (1, 0), (2, 1), (3, 1), (0, 2), (1, 3)],
        "2c/6887",
        "orthogonal",
        [1, 0, 0, 0, 0, 0, 0, 0],
        13_774,
        0.01,
    ),
    (&[(0, 1), (1, 0), (2, 0), (3, 1), (1, 2), (2, 2)], "6×3", "still", [0; 8], 1, 0.01),
    (&[(0, 0), (2, 1), (4, 0), (6, 1), (8, 0)], "c/3", "oblique", [0, 0, 0, 0, 0, 1, 0, 0], 6, 0.01),
];

/// A row of the list, as the lists of patterns have them: a picture, how the pattern moves,
/// the dial of the ways it went, two figures, a small button, and a share as a bar.
fn row(index: usize) -> impl Scene {
    let (cells, speed, way, ways, period, share) = PATTERNS[index];
    let row = Row(index);
    // As in a world that looks the same after a quarter turn: a pattern can go every way
    // that quarter turns take it from one it went.
    let possible = std::array::from_fn(|way| (0..4).any(|quarters| ways[(way + 2 * quarters) % 8] > 0));
    bsn! {
        list_row()
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(6),
        }
        template_value(row)
        on(|click: On<Pointer<Click>>, rows: Query<&Row>, mut gallery: ResMut<Gallery>| {
            if let Ok(&Row(index)) = rows.get(click.entity) {
                gallery.chosen = index;
            }
        })
        Children [
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(COLUMN_GAP),
                }
                template_value(Pickable::IGNORE)
                Children [
                    picture(cells),
                    (
                        Node {
                            flex_grow: 1.0,
                            flex_basis: px(0),
                            min_width: px(0),
                            flex_direction: FlexDirection::Column,
                            row_gap: px(2),
                        }
                        template_value(Pickable::IGNORE)
                        Children [
                            mono(speed, 14.0, palette::WHITE),
                            (caption(way) template_value(Pickable::IGNORE)),
                        ]
                    ),
                    dial(&ways, &possible, None),
                    number(period.to_string(), PERIOD_COLUMN, palette::LIGHT_GRAY_1),
                    number(cells.len().to_string(), CELLS_COLUMN, palette::LIGHT_GRAY_1),
                    // A click on the button is the button's: the row is not chosen by it.
                    icon_button(icons::LOOK, palette::LIGHT_GRAY_1),
                ]
            ),
            share_bar(share, Aspect::Pattern),
        ]
    }
}

/// The icons, each under its name.
fn icons_panel() -> impl Scene {
    let icons: Vec<_> = icons::ALL.into_iter().map(icon_tile).collect();
    bsn! {
        side_panel(520.0, bsn_list![
            panel_title(Aspect::View, "Icons"),
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: px(4),
                    row_gap: px(4),
                }
                Children [ {icons} ]
            ),
        ])
        Node { display: Display::Flex }
    }
}

fn icon_tile((name, glyph): (&'static str, &'static str)) -> impl Scene {
    bsn! {
        Node {
            width: px(78),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(4),
            padding: UiRect::axes(px(2), px(6)),
        }
        Children [
            icons::icon(glyph, 22.0, palette::LIGHT_GRAY_1),
            caption(name),
        ]
    }
}

/// A checkbox says what it would be, and whoever made it sees to it.
fn tick(change: On<ValueChange<bool>>, mut commands: Commands) {
    check(&mut commands, change.source, !change.value, change.value);
}

/// So does a slider.
fn slide(change: On<ValueChange<f32>>, mut commands: Commands) {
    commands.entity(change.source).insert(SliderValue(change.value));
}

/// A click turns a chip on or off.
fn flip(click: On<Pointer<Click>>, chips: Query<Has<Checked>>, mut commands: Commands) {
    if let Ok(on) = chips.get(click.entity) {
        check(&mut commands, click.entity, on, !on);
    }
}

/// The side panel is there or not.
fn show_panel(gallery: Res<Gallery>, mut panel: Single<&mut Node, With<Panel>>) {
    let display = if gallery.panel { Display::Flex } else { Display::None };
    if panel.display != display {
        panel.display = display;
    }
}

/// The row that was clicked last is outlined: the outline of a row is its owner's.
fn outline_chosen(gallery: Res<Gallery>, mut rows: Query<(&Row, &mut BorderColor)>) {
    for (&Row(index), mut border) in &mut rows {
        let color = if index == gallery.chosen { Aspect::Pattern.color() } else { Color::NONE };
        border.set_if_neq(BorderColor::all(color));
    }
}

/// The card of the sliders says where the first of them is.
fn measure(slider: Single<&SliderValue, With<Measured>>, mut text: Single<&mut Text, With<Measure>>) {
    text.set_if_neq(Text(format!("{:.0} %", 100.0 * slider.0)));
}

/// Nothing else paces the frames: what is left of a sixtieth of a second is waited out here.
fn pace(mut last: Local<Option<Instant>>) {
    const FRAME: Duration = Duration::from_micros(16_667);
    if let Some(spent) = last.map(|last| last.elapsed())
        && spent < FRAME
    {
        std::thread::sleep(FRAME - spent);
    }
    *last = Some(Instant::now());
}

/// After a few frames, a picture of the window; when it is saved, the gallery leaves.
fn take_picture(
    mut shot: ResMut<Shot>,
    screenshots: Query<(), With<Screenshot>>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    shot.frames += 1;
    if shot.frames == 30 {
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(shot.path.clone()));
    } else if shot.frames > 34 && screenshots.is_empty() {
        exit.write(AppExit::Success);
    }
}
