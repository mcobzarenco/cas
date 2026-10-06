//! Drawing a rule at random: the part of the rule editor that asks which properties the rule
//! should have, says how many rules have them all, and draws one.
//!
//! The properties are those a search goes through ([`cas_core::families`]), each a chip to
//! switch on. How many rules have them, and how many canonical rules, is counted on other
//! threads, which takes up to half a minute for the largest families; a family small enough is
//! kept, and its rules are then drawn evenly.

use std::sync::{
    Arc, Mutex,
    mpsc::{Receiver, Sender, channel},
};

use bevy::{
    feathers::{cursor::EntityCursor, palette},
    picking::hover::Hovered,
    prelude::*,
    ui::Checked,
    ui_widgets::{Activate, ValueChange},
    window::SystemCursorIcon,
};

use cas_core::{
    families::{COUNTABLE, Constraint, EVERY_RULE, Family, Progress, Size, Turn},
    rules::BlockRule,
    universe::{Rng, Universe},
};
use cas_ui::{
    Aspect, Sign, button, caption, check, checkbox, chip_box, group_digits, icons, section_title, tile_label,
};

use crate::{editor::RuleEditor, sim::SimSystems};

/// A family of at most so many rules is kept, to draw from evenly: among its rules, or among
/// its canonical rules.
const KEPT: u64 = 2_000_000;
/// So often a draw is tried that finds no rule: where any weighting will do, the one drawn
/// may have none.
const TRIES: usize = 20;

/// A property that may be asked for, as the flow shows it: the name the rig knows it by, its
/// sign, and a word or two (none where the sign says it all). The rule library narrows its
/// list by the same properties, with the same chips.
pub(crate) struct Chip {
    pub(crate) name: &'static str,
    pub(crate) constraint: Constraint,
    sign: Sign,
    label: &'static str,
}

const fn chip(name: &'static str, constraint: Constraint, sign: Sign, label: &'static str) -> Chip {
    Chip { name, constraint, sign, label }
}

/// The chips, group by group: the turns, the mirrors, what patterns keep, the table. The
/// mirrors across the diagonals are the mirror's icon, turned to lie along them.
pub(crate) const CHIPS: [Chip; 16] = [
    chip("quarter-turn", Constraint::Symmetric(Turn::Quarter), Sign::Written("90°"), ""),
    chip("half-turn", Constraint::Symmetric(Turn::Half), Sign::Written("180°"), ""),
    chip("mirror", Constraint::Symmetric(Turn::Mirror), Sign::Icon(icons::MIRROR), ""),
    chip("flip", Constraint::Symmetric(Turn::Flip), Sign::Icon(icons::FLIP), ""),
    chip("diagonal", Constraint::Symmetric(Turn::Diagonal), Sign::Turned(icons::MIRROR, -45.0), ""),
    chip("anti-diagonal", Constraint::Symmetric(Turn::AntiDiagonal), Sign::Turned(icons::MIRROR, 45.0), ""),
    chip("conserving", Constraint::Conserving, Sign::Icon(icons::CELLS), "cells"),
    chip("weighted", Constraint::Weighted(None), Sign::Icon(icons::WEIGHT), "a weight"),
    chip("parity", Constraint::Parity, Sign::Icon(icons::PARITY), "parity"),
    chip("momentum", Constraint::Momentum, Sign::Icon(icons::MOMENTUM), "momentum"),
    chip("turning", Constraint::Turning, Sign::Icon(icons::TURN), "turns blocks"),
    chip("linear", Constraint::Linear, Sign::Icon(icons::LINEAR), "linear"),
    chip("involution", Constraint::Involution, Sign::Icon(icons::INVERSE), "own inverse"),
    chip("complement", Constraint::Complement, Sign::Icon(icons::STATES), "states alike"),
    chip("stable-vacuum", Constraint::StableVacuum, Sign::Icon(icons::EMPTY), "empty stays empty"),
    chip("sparse", Constraint::Sparse(4), Sign::Written("≤4"), "blocks change"),
];
const TURNS: std::ops::Range<usize> = 0..2;
const MIRRORS: std::ops::Range<usize> = 2..6;
const KEEPS: std::ops::Range<usize> = 6..10;
const TABLE: std::ops::Range<usize> = 10..16;
/// The chip that takes a number.
const SPARSE: usize = 15;
/// The chips of which only one can be on: a rule that keeps a weight in the sense of the
/// second is one that does not keep the number of cells.
const EITHER: [usize; 2] = [6, 7];

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
    answers: Mutex<Receiver<(u64, Count)>>,
    reply: Sender<(u64, Count)>,
}

/// What is known of the family asked for.
enum Count {
    /// It is being counted on other threads, which say how far they have got.
    Counting(Arc<Progress>),
    /// It has more rules than are gone through, and nothing else says how many.
    Many,
    /// How many rules it has, and how many canonical rules; and while they are few, the rules
    /// themselves and the worlds they make: the canonical forms among them, each once.
    Known { size: Size, kept: Option<(Vec<BlockRule>, Vec<BlockRule>)> },
}

impl Default for Sampler {
    fn default() -> Self {
        let (reply, answers) = channel();
        Self {
            open: false,
            wanted: [false; CHIPS.len()],
            sparse: 4,
            canonical: false,
            count: Count::Known { size: EVERY_RULE, kept: None },
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

    /// Has the family counted, out of the way of the frames; a count asked for before is of
    /// no use any more, and stops.
    fn ask(&mut self) {
        self.asked += 1;
        if let Count::Counting(progress) = &self.count {
            progress.stop();
        }
        let family = self.family();
        if family.constraints().is_empty() {
            self.count = Count::Known { size: EVERY_RULE, kept: None };
            return;
        }
        let progress = Arc::new(Progress::default());
        self.count = Count::Counting(progress.clone());
        let (asked, reply) = (self.asked, self.reply.clone());
        std::thread::spawn(move || {
            // Half the cores: the world on the grid goes on running meanwhile.
            let threads = std::thread::available_parallelism().map_or(1, |n| n.get() / 2).max(1);
            let count = match family.size(threads, COUNTABLE, &progress) {
                _ if progress.stopped() => return,
                None => Count::Many,
                Some(size) if size.rules <= KEPT => {
                    let worlds = family.canonical_rules(threads, KEPT, &Progress::default());
                    Count::Known { size, kept: worlds.map(|(_, worlds)| (family.rules(), worlds)) }
                }
                Some(size) => Count::Known { size, kept: None },
            };
            // Nobody listens if the app has gone.
            let _ = reply.send((asked, count));
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
        app.init_resource::<Sampler>().add_systems(
            Update,
            (hear_counts, show.run_if(resource_changed::<Sampler>.or_else(counting)))
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
                                icons::icon(icons::OPENS, 12.0, palette::LIGHT_GRAY_2)
                                UiTransform
                                Chevron
                                template_value(Pickable::IGNORE)
                            ),
                            (section_title("RANDOM RULE") template_value(Pickable::IGNORE)),
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
                        button("Random")
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
                    tile_label("LOOKS THE SAME"),
                    (
                        row()
                        Children [
                            words("turned"),
                            { turns },
                            words("mirrored"),
                            { mirrors },
                        ]
                    ),
                    tile_label("CONSERVES"),
                    (row() Children [ { keeps } ]),
                    tile_label("THE TABLE"),
                    (
                        row()
                        Children [
                            { table },
                            step("SparseLess", icons::LESS, -1),
                            step("SparseMore", icons::MORE, 1),
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
                                Node { flex_shrink: 0.0 }
                                CanonicalBox
                                on(|change: On<ValueChange<bool>>, mut sampler: ResMut<Sampler>| {
                                    sampler.canonical = change.value;
                                })
                            ),
                            (
                                // A count of trillions takes two lines, on its own side.
                                #RandomCount
                                caption("")
                                Node { flex_grow: 1.0, flex_basis: px(0), min_width: px(0) }
                                TextLayout { justify: Justify::Right }
                                Counted
                            ),
                        ]
                    ),
                    caption("A rule turned, mirrored or begun later in its vacuum's cycle is another table; the canonical form is the first of them."),
                ]
            ),
        ]
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

fn chip_scene(index: usize) -> impl Scene {
    let name = Name::new(format!("Want:{}", CHIPS[index].name));
    let want = Want(index);
    bsn! {
        cas_ui::chip_marked(CHIPS[index].sign, CHIPS[index].label, Aspect::Rule, WantSign(index))
        template_value(name)
        template_value(want)
        on(|click: On<Pointer<Click>>, chips: Query<&Want>, mut sampler: ResMut<Sampler>| {
            if let Ok(&Want(index)) = chips.get(click.entity) {
                sampler.wanted[index] = !sampler.wanted[index];
                // The number of cells or a weight in its place: asking for one lets go of
                // the other.
                if sampler.wanted[index] && EITHER.contains(&index) {
                    for other in EITHER.into_iter().filter(|other| *other != index) {
                        sampler.wanted[other] = false;
                    }
                }
                sampler.ask();
            }
        })
    }
}

/// The chip of a property, by its place in [`CHIPS`], as the library shows it too.
pub(crate) fn chip_face(index: usize) -> impl Scene {
    cas_ui::chip(CHIPS[index].sign, CHIPS[index].label, Aspect::Rule)
}

/// A small button that makes the number of the sparse chip one less or one more.
fn step(name: &'static str, sign: &'static str, by: i8) -> impl Scene {
    let name = Name::new(name);
    bsn! {
        chip_box(Aspect::Rule)
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
            icons::icon(sign, 12.0, palette::LIGHT_GRAY_1)
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
        // In canonical form every world is as likely as any other; otherwise every table is.
        Count::Known { kept: Some((rules, worlds)), .. } => {
            let from = if sampler.canonical { worlds } else { rules };
            (!from.is_empty()).then(|| from[(rng.next_u64() % from.len().max(1) as u64) as usize].clone())
        }
        _ => {
            let drawn = (0..TRIES).find_map(|_| family.draw(&mut rng));
            drawn.map(|rule| if sampler.canonical { rule.canonical() } else { rule })
        }
    };
    let Some(rule) = rule else {
        let why = match sampler.count {
            Count::Counting(_) => "No rule drawn: the rules with all of this are still being counted.",
            _ => "No rule has all of this.",
        };
        editor.say(why, universe.rule());
        return;
    };
    universe.set_rule(rule);
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

/// Whether a count is on its way, which the flow says how far it has got with.
fn counting(sampler: Res<Sampler>) -> bool {
    matches!(sampler.count, Count::Counting(_))
}

/// Takes the counts as they come in; only the answer to the last question counts.
fn hear_counts(mut sampler: ResMut<Sampler>) {
    let answers: Vec<(u64, Count)> =
        sampler.answers.lock().map_or_else(|_| Vec::new(), |answers| answers.try_iter().collect());
    for (asked, count) in answers {
        if asked == sampler.asked {
            sampler.count = count;
        }
    }
}

/// Keeps the flow showing what is asked for and what is known.
fn show(
    sampler: Res<Sampler>,
    mut body: Single<&mut Node, With<Body>>,
    mut chevron: Single<&mut UiTransform, With<Chevron>>,
    chips: Query<(Entity, &Want, Has<Checked>)>,
    mut signs: Query<(&WantSign, &mut Text), (Without<FamilyName>, Without<Counted>)>,
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
    // A chip that is asked for is on: the kit outlines it and brightens its sign.
    for (chip, &Want(index), on) in &chips {
        check(&mut commands, chip, on, sampler.wanted[index]);
    }
    for (&WantSign(index), mut text) in &mut signs {
        if index == SPARSE {
            text.set_if_neq(Text(format!("≤{}", sampler.sparse)));
        }
    }
    let family = sampler.family();
    let asked_for = match family.constraints() {
        [] => "any rule there is".to_string(),
        _ => named(&family),
    };
    family_name.set_if_neq(Text(asked_for));
    // A count of trillions is more than the line holds: it breaks between its numbers, not
    // within one.
    let whole = |count: u64| group_digits(count).replace(' ', "\u{a0}");
    let so_many = |count: u64, one: &str| format!("{} {one}{}", whole(count), if count == 1 { "" } else { "s" });
    let count = match &sampler.count {
        Count::Counting(progress) => match progress.rules() {
            0 => "counting…".to_string(),
            gone => format!("counting… {} rules so far", whole(gone)),
        },
        Count::Many => format!("more than {} rules", whole(COUNTABLE)),
        Count::Known { size, .. } if size.rules == 0 => "no rule has all of this".to_string(),
        // In canonical form it is the canonical rules that are drawn from.
        Count::Known { size: Size { canonical: Some(canonical), .. }, .. } if sampler.canonical => {
            so_many(*canonical, "canonical rule")
        }
        Count::Known { size: Size { rules, canonical: Some(canonical) }, .. } => {
            format!("{} · {} canonical", so_many(*rules, "rule"), whole(*canonical))
        }
        Count::Known { size: Size { rules, canonical: None }, .. } => {
            format!("{} · too many to count the canonical ones", so_many(*rules, "rule"))
        }
    };
    counted.set_if_neq(Text(count));
    let (checkbox, checked) = *canonical;
    check(&mut commands, checkbox, checked, sampler.canonical);
}
