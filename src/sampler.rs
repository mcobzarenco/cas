//! Drawing a rule at random: the part of the rule editor that asks which properties the rule
//! should have, says how many rules have them all, and draws one.
//!
//! The properties are those a search goes through ([`cas_core::families`]), each a chip to
//! switch on. How many rules have them is counted in the background, since some families take
//! a second to go through; a family small enough is kept, and its rules are then drawn evenly.

use std::sync::{
    Mutex,
    mpsc::{Receiver, Sender, channel},
};

use bevy::{
    feathers::{
        constants::fonts,
        controls::FeathersButton,
        cursor::EntityCursor,
        palette,
        theme::{ThemeTextColor, ThemedText},
        tokens,
    },
    picking::hover::Hovered,
    prelude::*,
    text::{FontSourceTemplate, FontWeight, LetterSpacing},
    ui::Checked,
    ui_widgets::{Activate, ValueChange},
    window::SystemCursorIcon,
};

use cas_core::{
    families::{self, Constraint, Family, Turn},
    rules::BlockRule,
    universe::{Rng, Universe},
};

use crate::{
    editor::RuleEditor,
    sim::SimSystems,
    ui::{Aspect, caption, checkbox, group_digits},
};

/// Rules are counted up to so many; of a family with more, that is all that is said.
const COUNTED: usize = 1_000_000;
/// A family of at most so many rules is kept, to draw from evenly and to count its worlds.
const KEPT: usize = 300_000;
/// So often a draw is tried that finds no rule: where any weighting will do, the one drawn
/// may have none.
const TRIES: usize = 20;

/// A property that may be asked for, as the flow shows it: the name the rig knows it by, its
/// sign in the mono font, and a word or two (none where the sign says it all).
struct Chip {
    name: &'static str,
    constraint: Constraint,
    sign: &'static str,
    label: &'static str,
}

const fn chip(name: &'static str, constraint: Constraint, sign: &'static str, label: &'static str) -> Chip {
    Chip { name, constraint, sign, label }
}

/// The chips, group by group: the turns, the mirrors, what patterns keep, the table.
const CHIPS: [Chip; 16] = [
    chip("quarter-turn", Constraint::Symmetric(Turn::Quarter), "90°", ""),
    chip("half-turn", Constraint::Symmetric(Turn::Half), "180°", ""),
    chip("mirror", Constraint::Symmetric(Turn::Mirror), "│", ""),
    chip("flip", Constraint::Symmetric(Turn::Flip), "─", ""),
    chip("diagonal", Constraint::Symmetric(Turn::Diagonal), "╲", ""),
    chip("anti-diagonal", Constraint::Symmetric(Turn::AntiDiagonal), "╱", ""),
    chip("conserving", Constraint::Conserving, "Σ■", "cells"),
    chip("weighted", Constraint::Weighted(None), "Σw", "a weight"),
    chip("parity", Constraint::Parity, "±", "parity"),
    chip("momentum", Constraint::Momentum, "Σ→", "momentum"),
    chip("turning", Constraint::Turning, "◧→◨", "turns blocks"),
    chip("linear", Constraint::Linear, "A+B", "linear"),
    chip("involution", Constraint::Involution, "←→", "own inverse"),
    chip("complement", Constraint::Complement, "■↔□", "states alike"),
    chip("stable-vacuum", Constraint::StableVacuum, "□→□", "empty stays empty"),
    chip("sparse", Constraint::Sparse(4), "≤4", "blocks change"),
];
const TURNS: std::ops::Range<usize> = 0..2;
const MIRRORS: std::ops::Range<usize> = 2..6;
const KEEPS: std::ops::Range<usize> = 6..10;
const TABLE: std::ops::Range<usize> = 10..16;
/// The chip that takes a number.
const SPARSE: usize = 15;

#[derive(Resource)]
pub struct Sampler {
    open: bool,
    /// Which chips are on.
    wanted: [bool; CHIPS.len()],
    /// At most so many blocks may change, where that is asked for.
    sparse: u8,
    /// Whether the rule drawn is put in canonical form.
    canonical: bool,
    count: Count,
    /// How many counts were asked for: tells the answer to the last one from the others.
    asked: u64,
    answers: Mutex<Receiver<(u64, Known)>>,
    reply: Sender<(u64, Known)>,
}

/// What is known of the family asked for.
enum Count {
    Counting,
    Known(Known),
}

struct Known {
    /// How many rules it has, unless that is more than [`COUNTED`].
    tables: Option<usize>,
    /// The rules themselves, and how many worlds they make, for a family small enough.
    kept: Option<(Vec<BlockRule>, usize)>,
}

impl Default for Sampler {
    fn default() -> Self {
        let (reply, answers) = channel();
        Self {
            open: false,
            wanted: [false; CHIPS.len()],
            sparse: 4,
            canonical: false,
            count: Count::Known(Known { tables: None, kept: None }),
            asked: 0,
            answers: Mutex::new(answers),
            reply,
        }
    }
}

impl Sampler {
    /// The family asked for.
    fn family(&self) -> Family {
        let wanted = CHIPS.iter().zip(self.wanted).filter(|(_, wanted)| *wanted);
        Family::new(wanted.map(|(chip, _)| match chip.constraint {
            Constraint::Sparse(_) => Constraint::Sparse(self.sparse),
            constraint => constraint,
        }))
    }

    /// Has the family counted, out of the way of the frames.
    fn ask(&mut self) {
        self.asked += 1;
        let family = self.family();
        if family.constraints().is_empty() {
            self.count = Count::Known(Known { tables: None, kept: None });
            return;
        }
        self.count = Count::Counting;
        let (asked, reply) = (self.asked, self.reply.clone());
        std::thread::spawn(move || {
            let tables = family.count(COUNTED);
            let kept = tables.filter(|tables| *tables <= KEPT).map(|_| {
                let rules = family.rules();
                let worlds = families::distinct(rules.iter().cloned()).len();
                (rules, worlds)
            });
            // Nobody listens if the app has gone.
            let _ = reply.send((asked, Known { tables, kept }));
        });
    }
}

/// The part that opens and closes.
#[derive(Component, Default, Clone)]
struct Body;

/// The mark that turns when it opens.
#[derive(Component, Default, Clone)]
struct Chevron;

/// The family asked for, in a word, next to the title.
#[derive(Component, Default, Clone)]
struct FamilyName;

/// How many rules have all that is asked for.
#[derive(Component, Default, Clone)]
struct Counted;

/// A chip, by its place in [`CHIPS`], and its sign.
#[derive(Component, Default, Clone, Copy)]
struct Want(usize);

#[derive(Component, Default, Clone, Copy)]
struct WantSign(usize);

/// The checkbox for the canonical form.
#[derive(Component, Default, Clone)]
struct CanonicalBox;

pub struct SamplerPlugin;

impl Plugin for SamplerPlugin {
    fn build(&self, app: &mut App) {
        let chip_hovered = |chips: Query<(), (With<Want>, Changed<Hovered>)>| !chips.is_empty();
        app.init_resource::<Sampler>().add_systems(
            Update,
            (
                hear_counts,
                show.run_if(resource_changed::<Sampler>.or_eager(chip_hovered)),
            )
                .chain()
                .in_set(SimSystems::Present),
        );
    }
}

/// The flow, as a section of the rule editor.
pub fn sampler_section() -> impl Scene {
    let chips = |range: std::ops::Range<usize>| range.map(chip_scene).collect::<Vec<_>>();
    let (turns, mirrors, keeps, table) = (chips(TURNS), chips(MIRRORS), chips(KEEPS), chips(TABLE));
    bsn! {
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(8),
        }
        Children [
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(8),
                }
                Children [
                    (
                        // The title opens and closes the flow.
                        #RandomWith
                        Node {
                            flex_direction: FlexDirection::Row,
                            align_items: AlignItems::Center,
                            column_gap: px(6),
                            flex_grow: 1.0,
                            flex_basis: px(0),
                            min_width: px(0),
                        }
                        Hovered
                        EntityCursor::System(SystemCursorIcon::Pointer)
                        on(|_: On<Pointer<Click>>, mut sampler: ResMut<Sampler>| sampler.open = !sampler.open)
                        Children [
                            (
                                Text("›")
                                TextFont {
                                    font: FontSourceTemplate::Handle(fonts::BOLD),
                                    font_size: FontSize::Px(14.0),
                                    weight: FontWeight::BOLD,
                                }
                                ThemeTextColor(tokens::TEXT_DIM)
                                UiTransform
                                Chevron
                                template_value(Pickable::IGNORE)
                            ),
                            (
                                Text("RANDOM RULE")
                                TextFont {
                                    font: FontSourceTemplate::Handle(fonts::BOLD),
                                    font_size: FontSize::Px(11.0),
                                    weight: FontWeight::BOLD,
                                }
                                ThemeTextColor(tokens::TEXT_DIM)
                                template_value(Pickable::IGNORE)
                            ),
                            (
                                #RandomFamily
                                caption("")
                                FamilyName
                                Node { flex_grow: 1.0, flex_basis: px(0) }
                                template_value(Pickable::IGNORE)
                            ),
                        ]
                    ),
                    (
                        #RuleRandom
                        @FeathersButton {
                            @caption: bsn! { Text("Random") ThemedText }
                        }
                        Node { flex_shrink: 0.0 }
                        on(draw)
                    ),
                ]
            ),
            (
                Node {
                    display: Display::None,
                    flex_direction: FlexDirection::Column,
                    row_gap: px(6),
                }
                Body
                Children [
                    heading("LOOKS THE SAME"),
                    (
                        row()
                        Children [
                            words("turned"),
                            { turns },
                            words("mirrored"),
                            { mirrors },
                        ]
                    ),
                    heading("PATTERNS KEEP"),
                    (row() Children [ { keeps } ]),
                    heading("THE TABLE"),
                    (
                        row()
                        Children [
                            { table },
                            step("SparseLess", "−", -1),
                            step("SparseMore", "+", 1),
                        ]
                    ),
                    (
                        Node {
                            flex_direction: FlexDirection::Row,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::SpaceBetween,
                            column_gap: px(8),
                            margin: UiRect::top(px(2)),
                        }
                        Children [
                            (
                                checkbox("In canonical form", "RandomCanonical", Aspect::Rule, "")
                                CanonicalBox
                                on(|change: On<ValueChange<bool>>, mut sampler: ResMut<Sampler>| {
                                    sampler.canonical = change.value;
                                })
                            ),
                            (#RandomCount caption("") Counted),
                        ]
                    ),
                ]
            ),
        ]
    }
}

/// The name of a group of chips.
fn heading(name: &'static str) -> impl Scene {
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

/// A line of chips, going on to the next when it is full.
fn row() -> impl Scene {
    bsn! {
        Node {
            flex_direction: FlexDirection::Row,
            flex_wrap: FlexWrap::Wrap,
            align_items: AlignItems::Center,
            column_gap: px(5),
            row_gap: px(5),
        }
    }
}

/// A word between chips.
fn words(text: &'static str) -> impl Scene {
    bsn! {
        caption(text)
        Node { margin: UiRect::horizontal(px(2)) }
    }
}

/// The box of a chip or of a step: a small button that lights up under the pointer.
fn chip_box() -> impl Scene {
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
    }
}

fn chip_scene(index: usize) -> impl Scene {
    let chip = &CHIPS[index];
    let name = Name::new(format!("Want:{}", chip.name));
    let (want, sign) = (Want(index), WantSign(index));
    // A sign alone needs no word next to it, and less room around it.
    let (label, sides) = if chip.label.is_empty() { (Display::None, 5.0) } else { (Display::Flex, 7.0) };
    bsn! {
        chip_box()
        Node { padding: UiRect::axes(px(sides), px(3)) }
        template_value(name)
        template_value(want)
        on(|click: On<Pointer<Click>>, chips: Query<&Want>, mut sampler: ResMut<Sampler>| {
            if let Ok(&Want(index)) = chips.get(click.entity) {
                sampler.wanted[index] = !sampler.wanted[index];
                sampler.ask();
            }
        })
        Children [
            (
                Text({chip.sign})
                TextFont {
                    font: FontSourceTemplate::Handle(fonts::MONO),
                    font_size: FontSize::Px(13.0),
                    weight: FontWeight::NORMAL,
                }
                TextColor(palette::LIGHT_GRAY_2)
                template_value(sign)
                template_value(Pickable::IGNORE)
            ),
            (
                Text({chip.label})
                TextFont {
                    font: FontSourceTemplate::Handle(fonts::REGULAR),
                    font_size: FontSize::Px(12.0),
                    weight: FontWeight::NORMAL,
                }
                TextColor(palette::LIGHT_GRAY_1)
                Node { display: {label} }
                template_value(Pickable::IGNORE)
            ),
        ]
    }
}

/// A small button that makes the number of the sparse chip one less or one more.
fn step(name: &'static str, sign: &'static str, by: i8) -> impl Scene {
    let name = Name::new(name);
    bsn! {
        chip_box()
        template_value(name)
        on(move |_: On<Pointer<Click>>, mut sampler: ResMut<Sampler>| {
            let sparse = sampler.sparse.saturating_add_signed(by).clamp(2, 16);
            if sparse != sampler.sparse {
                sampler.sparse = sparse;
                if sampler.wanted[SPARSE] {
                    sampler.ask();
                }
            }
        })
        Children [(
            Text(sign)
            TextFont {
                font: FontSourceTemplate::Handle(fonts::MONO),
                font_size: FontSize::Px(12.0),
                weight: FontWeight::NORMAL,
            }
            TextColor(palette::LIGHT_GRAY_1)
            template_value(Pickable::IGNORE)
        )]
    }
}

/// Draws a rule with all that is asked for, evenly from a family that was kept and by
/// filling in a table at random otherwise.
fn draw(
    _: On<Activate>,
    sampler: Res<Sampler>,
    mut rng: ResMut<Rng>,
    mut universe: ResMut<Universe>,
    mut editor: ResMut<RuleEditor>,
) {
    let family = sampler.family();
    let rule = match &sampler.count {
        Count::Known(Known { tables: Some(0), .. }) => None,
        Count::Known(Known { kept: Some((rules, _)), .. }) => {
            Some(rules[(rng.next_u64() % rules.len() as u64) as usize].clone())
        }
        _ => (0..TRIES).find_map(|_| family.draw(&mut rng)),
    };
    let Some(rule) = rule else {
        let why = match sampler.count {
            Count::Counting => "No rule drawn: the rules with all of this are still being counted.",
            Count::Known(_) => "No rule has all of this.",
        };
        editor.say(why, universe.rule());
        return;
    };
    universe.set_rule(if sampler.canonical { rule.canonical() } else { rule });
    let what = match (family.constraints(), sampler.canonical) {
        ([], false) => "A random permutation.".to_string(),
        ([], true) => "A random permutation, in canonical form.".to_string(),
        (_, false) => format!("A random rule that is {}.", named(&family)),
        (_, true) => format!("A random rule that is {}, in canonical form.", named(&family)),
    };
    editor.say(what, universe.rule());
}

/// The family by the names of its properties.
fn named(family: &Family) -> String {
    let names: Vec<String> = family.constraints().iter().map(|constraint| constraint.to_string()).collect();
    names.join(" + ")
}

/// Takes the counts as they come in; only the answer to the last question counts.
fn hear_counts(mut sampler: ResMut<Sampler>) {
    let answers: Vec<(u64, Known)> = sampler.answers.lock().map_or_else(|_| Vec::new(), |answers| answers.try_iter().collect());
    for (asked, known) in answers {
        if asked == sampler.asked {
            sampler.count = Count::Known(known);
        }
    }
}

/// Keeps the flow showing what is asked for and what is known.
fn show(
    sampler: Res<Sampler>,
    mut body: Single<&mut Node, With<Body>>,
    mut chevron: Single<&mut UiTransform, With<Chevron>>,
    mut chips: Query<(&Want, &Hovered, &mut BackgroundColor, &mut BorderColor)>,
    mut signs: Query<(&WantSign, &mut Text, &mut TextColor), (Without<FamilyName>, Without<Counted>)>,
    mut family_name: Single<&mut Text, (With<FamilyName>, Without<Counted>, Without<WantSign>)>,
    mut counted: Single<&mut Text, (With<Counted>, Without<FamilyName>, Without<WantSign>)>,
    canonical: Single<(Entity, Has<Checked>), With<CanonicalBox>>,
    mut commands: Commands,
) {
    let display = if sampler.open { Display::Flex } else { Display::None };
    if body.display != display {
        body.display = display;
    }
    // The mark points down while the flow is open.
    let turned = if sampler.open { Rot2::FRAC_PI_2 } else { Rot2::IDENTITY };
    if chevron.rotation != turned {
        chevron.rotation = turned;
    }
    for (&Want(index), hovered, mut fill, mut border) in &mut chips {
        let color = if hovered.0 { palette::GRAY_3 } else { palette::GRAY_2 };
        fill.set_if_neq(BackgroundColor(color));
        let color = if sampler.wanted[index] { Aspect::Rule.color() } else { Color::NONE };
        border.set_if_neq(BorderColor::all(color));
    }
    for (&WantSign(index), mut text, mut color) in &mut signs {
        if index == SPARSE {
            text.set_if_neq(Text(format!("≤{}", sampler.sparse)));
        }
        let ink = if sampler.wanted[index] { palette::WHITE } else { palette::LIGHT_GRAY_2 };
        color.set_if_neq(TextColor(ink));
    }
    let family = sampler.family();
    let asked_for = match family.constraints() {
        [] => "any rule there is".to_string(),
        _ => named(&family),
    };
    family_name.set_if_neq(Text(asked_for));
    let count = match &sampler.count {
        _ if family.constraints().is_empty() => "16! rules".to_string(),
        Count::Counting => "counting…".to_string(),
        Count::Known(Known { tables: Some(0), .. }) => "no rule has all of this".to_string(),
        Count::Known(Known { tables: Some(tables), kept: Some((_, worlds)) }) => {
            format!("{} rules, {} worlds", group_digits(*tables as i64), group_digits(*worlds as i64))
        }
        Count::Known(Known { tables: Some(tables), kept: None }) => format!("{} rules", group_digits(*tables as i64)),
        Count::Known(Known { tables: None, .. }) => format!("more than {} rules", group_digits(COUNTED as i64)),
    };
    counted.set_if_neq(Text(count));
    let (checkbox, checked) = *canonical;
    match (sampler.canonical, checked) {
        (true, false) => commands.entity(checkbox).insert(Checked),
        (false, true) => commands.entity(checkbox).remove::<Checked>(),
        _ => return,
    };
}
