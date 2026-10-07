//! The properties a rule is asked to have, and the rules drawn at random with them: the part
//! of the rule library that narrows its list to the rules that have some properties, says how
//! many rules have them all, and draws a sample of those.
//!
//! The properties are those a search goes through ([`cas_core::families`]), each a chip to
//! switch on. How many rules have them, and how many canonical rules, is counted on other
//! threads while a sample is asked for, which takes up to half a minute for the largest
//! families; a family small enough is kept, and its rules are then drawn evenly.

use std::{
    collections::HashSet,
    sync::{
        Arc, Mutex,
        mpsc::{Receiver, Sender, channel},
    },
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
    universe::Rng,
};
use cas_ui::{Aspect, Sign, button, caption, check, checkbox, chip_box, group_digits, heading, icons, tile_label};

use crate::{library::RuleLibrary, sim::SimSystems};

/// A family of at most so many rules is kept, to draw from evenly: among its rules, or among
/// its canonical rules.
const KEPT: u64 = 2_000_000;
/// So many draws are tried for every rule of a sample that is filled in at random, before the
/// sample is left shorter: a draw may find a rule drawn already, or none at all where any
/// weighting will do and the one drawn has no rule of its own.
const TRIES: usize = 20;
/// How many rules a sample may be of, each as its chip writes it.
const SIZES: [(usize, &str); 3] = [(10, "10"), (30, "30"), (100, "100")];

/// A property that may be asked for, as the panel shows it: the name the rig knows it by, its
/// sign, and a word or two (none where the sign says it all).
pub(crate) struct Chip {
    pub(crate) name: &'static str,
    pub(crate) constraint: Constraint,
    sign: Sign,
    label: &'static str,
}

const fn chip(name: &'static str, constraint: Constraint, sign: Sign, label: &'static str) -> Chip {
    Chip { name, constraint, sign, label }
}

/// The chips, group by group: the turns, the mirrors, what patterns keep, the table, and how
/// the rule runs backwards: as itself, as itself turned or mirrored, with dead and alive
/// exchanged or not. The mirrors across the diagonals are the mirror's icon, turned to lie
/// along them.
pub(crate) const CHIPS: [Chip; 23] = [
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
    chip("complement", Constraint::Complement, Sign::Icon(icons::STATES), "states alike"),
    chip("stable-vacuum", Constraint::StableVacuum, Sign::Icon(icons::EMPTY), "empty stays empty"),
    chip("sparse", Constraint::Sparse(4), Sign::Written("≤4"), "blocks change"),
    chip("involution", Constraint::INVOLUTION, Sign::Icon(icons::EQUAL), "the same"),
    chip("inverse=quarter-turn", inverse(Turn::Quarter), Sign::Written("90°"), ""),
    chip("inverse=half-turn", inverse(Turn::Half), Sign::Written("180°"), ""),
    chip("inverse=mirror", inverse(Turn::Mirror), Sign::Icon(icons::MIRROR), ""),
    chip("inverse=flip", inverse(Turn::Flip), Sign::Icon(icons::FLIP), ""),
    chip("inverse=diagonal", inverse(Turn::Diagonal), Sign::Turned(icons::MIRROR, -45.0), ""),
    chip("inverse=anti-diagonal", inverse(Turn::AntiDiagonal), Sign::Turned(icons::MIRROR, 45.0), ""),
    chip(
        "inverse=complemented",
        Constraint::Inverse { through: None, complemented: true },
        Sign::Icon(icons::STATES),
        "complemented",
    ),
];

/// Run backwards, the rule is itself seen through the turn or mirror.
const fn inverse(through: Turn) -> Constraint {
    Constraint::Inverse { through: Some(through), complemented: false }
}

const TURNS: std::ops::Range<usize> = 0..2;
const MIRRORS: std::ops::Range<usize> = 2..6;
const KEEPS: std::ops::Range<usize> = 6..10;
const TABLE: std::ops::Range<usize> = 10..15;
/// The chip that takes a number.
const SPARSE: usize = 14;
/// The chips of which only one can be on: a rule that keeps a weight in the sense of the
/// second is one that does not keep the number of cells.
const EITHER: [usize; 2] = [6, 7];
/// The chips for how the rule runs backwards: as itself or as itself seen through one turn or
/// mirror, of which only one can be on, and with dead and alive exchanged, which goes with
/// any of them or alone. Together they make one property.
const REVERSAL: std::ops::Range<usize> = 15..23;
const REVERSAL_SEEN: std::ops::Range<usize> = 15..22;
const REVERSAL_SAME: usize = 15;
const REVERSAL_TURNS: std::ops::Range<usize> = 16..18;
const REVERSAL_MIRRORS: std::ops::Range<usize> = 18..22;
const REVERSAL_COMPLEMENTED: usize = 22;

#[derive(Resource)]
pub struct Sampler {
    /// Whether the chips are unfolded.
    open: bool,
    /// Which chips are on.
    wanted: [bool; CHIPS.len()],
    /// At most so many blocks may change, where that is asked for.
    sparse: u8,
    /// Whether the rules drawn are in canonical form: one for every world, each as likely as
    /// any other.
    canonical: bool,
    /// How many rules a sample is of: a place in [`SIZES`].
    size: usize,
    /// The rules drawn last, each once, in the order of their tables.
    sample: Vec<BlockRule>,
    /// Whether a sample was drawn for what is asked for now; and whether with the family
    /// counted, which is the best that can be drawn: one drawn while the family was being
    /// counted is drawn again when it is, if the family is then kept.
    drawn: bool,
    settled: bool,
    /// Whether the sample is every rule there is with all that is asked for.
    whole: bool,
    count: Count,
    /// The family the count is of.
    counted: Family,
    /// How many counts were asked for: tells the answer to the last one from the others.
    asked: u64,
    answers: Mutex<Receiver<(u64, Count)>>,
    reply: Sender<(u64, Count)>,
    /// Counts the changes the lists follow: of what is asked for, and of the sample.
    revision: u64,
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
            size: 0,
            sample: Vec::new(),
            drawn: false,
            settled: false,
            whole: false,
            count: Count::Known { size: EVERY_RULE, kept: None },
            counted: Family::default(),
            asked: 0,
            answers: Mutex::new(answers),
            reply,
            revision: 0,
        }
    }
}

impl Sampler {
    /// The family asked for: the rules with every property whose chip is on. The chips for
    /// how the rule runs backwards make one property between them.
    pub(crate) fn family(&self) -> Family {
        let on = |index: &usize| self.wanted[*index];
        let plain = (0..REVERSAL.start).filter(on).map(|index| match CHIPS[index].constraint {
            Constraint::Sparse(_) => Constraint::Sparse(self.sparse),
            constraint => constraint,
        });
        let seen = REVERSAL_SEEN.clone().find(on);
        let complemented = self.wanted[REVERSAL_COMPLEMENTED];
        let inverse = (seen.is_some() || complemented).then(|| match seen.map(|index| CHIPS[index].constraint) {
            Some(Constraint::Inverse { through, .. }) => Constraint::Inverse { through, complemented },
            _ => Constraint::Inverse { through: None, complemented },
        });
        Family::new(plain.chain(inverse))
    }

    /// The rules drawn last, in the order of their tables.
    pub(crate) fn sample(&self) -> &[BlockRule] {
        &self.sample
    }

    /// Whether the sample is every rule there is with all that is asked for.
    pub(crate) fn whole(&self) -> bool {
        self.whole
    }

    /// Whether the rules drawn are in canonical form.
    pub(crate) fn in_canonical_form(&self) -> bool {
        self.canonical
    }

    /// Whether no rule has all that is asked for, as far as is known.
    pub(crate) fn none_at_all(&self) -> bool {
        matches!(self.known(), Some(Count::Known { size: Size { rules: 0, .. }, .. }))
    }

    /// What tells a change of what is asked for, or of the sample, from none.
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    /// Asks for a property, or no longer. The number of cells or a weight in its place, and
    /// one way of seeing the rule run backwards or another: asking for one lets go of the
    /// others.
    fn want(&mut self, index: usize) {
        self.wanted[index] = !self.wanted[index];
        if self.wanted[index] {
            let others: Vec<usize> = match index {
                _ if EITHER.contains(&index) => EITHER.to_vec(),
                _ if REVERSAL_SEEN.contains(&index) => REVERSAL_SEEN.collect(),
                _ => Vec::new(),
            };
            for other in others.into_iter().filter(|other| *other != index) {
                self.wanted[other] = false;
            }
        }
        self.changed();
    }

    fn set_canonical(&mut self, canonical: bool) {
        if self.canonical != canonical {
            self.canonical = canonical;
            self.changed();
        }
    }

    /// What is asked for changed: the sample was of other rules, and the lists are to follow.
    fn changed(&mut self) {
        self.sample.clear();
        self.drawn = false;
        self.settled = false;
        self.whole = false;
        self.revision += 1;
    }

    /// Whether a sample is to be drawn: there is none for what is asked for, or the one
    /// there is was drawn while the family was being counted, and the family is kept now.
    fn wants_sample(&self) -> bool {
        !self.drawn || (!self.settled && matches!(self.known(), Some(Count::Known { kept: Some(_), .. })))
    }

    /// What is known of the family asked for now: nothing, where what was counted was another.
    fn known(&self) -> Option<&Count> {
        (self.counted == self.family()).then_some(&self.count)
    }

    /// Has the family counted, out of the way of the frames; a count asked for before is of
    /// no use any more, and stops.
    fn ask(&mut self) {
        self.asked += 1;
        if let Count::Counting(progress) = &self.count {
            progress.stop();
        }
        let family = self.family();
        self.counted = family.clone();
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
                    // Listed in a second pass, which a count no longer asked for stops too.
                    let Some((_, worlds)) = family.canonical_rules(threads, KEPT, &progress) else {
                        return;
                    };
                    Count::Known { size, kept: Some((family.rules(), worlds)) }
                }
                Some(size) => Count::Known { size, kept: None },
            };
            // Nobody listens if the app has gone.
            let _ = reply.send((asked, count));
        });
    }

    /// Takes the counts as they come in; only the answer to the last question counts. True if
    /// one did.
    fn hear(&mut self) -> bool {
        let answers: Vec<(u64, Count)> =
            self.answers.lock().map_or_else(|_| Vec::new(), |answers| answers.try_iter().collect());
        let mut heard = false;
        for (asked, count) in answers {
            if asked == self.asked {
                self.count = count;
                heard = true;
            }
        }
        heard
    }

    /// Draws the sample: so many rules with all that is asked for, each once, in the order of
    /// their tables. Evenly from a family that was kept, all of it when it is no larger than
    /// the sample; by filling in tables at random otherwise, which is not even, until the
    /// draws give nothing new for a while.
    fn generate(&mut self, rng: &mut Rng) {
        let (want, _) = SIZES[self.size];
        let kept = match self.known() {
            // In canonical form every world is as likely as any other; otherwise every table is.
            Some(Count::Known { kept: Some((rules, worlds)), .. }) => Some(if self.canonical { worlds } else { rules }),
            _ => None,
        };
        let (mut drawn, whole) = match kept {
            Some(from) => (some_of(from, want, rng), from.len() <= want),
            None => (self.filled(want, rng), false),
        };
        drawn.sort_by(|a, b| a.table().cmp(b.table()));
        self.settled = !matches!(self.known(), None | Some(Count::Counting(_)));
        self.sample = drawn;
        self.drawn = true;
        self.whole = whole;
        self.revision += 1;
    }

    /// So many different rules with all that is asked for, each filled in at random: fewer,
    /// if the draws give nothing new for a while.
    fn filled(&self, want: usize, rng: &mut Rng) -> Vec<BlockRule> {
        let family = self.family();
        let mut seen: HashSet<BlockRule> = HashSet::new();
        let mut tries = 0;
        while seen.len() < want && tries < TRIES * want {
            tries += 1;
            if let Some(rule) = family.draw(rng) {
                seen.insert(if self.canonical { rule.canonical() } else { rule });
            }
        }
        seen.into_iter().collect()
    }
}

/// So many of the rules, each at most once and each as likely as any other: all of them where
/// there are no more than that.
fn some_of(rules: &[BlockRule], count: usize, rng: &mut Rng) -> Vec<BlockRule> {
    if rules.len() <= count {
        return rules.to_vec();
    }
    let mut places: HashSet<usize> = HashSet::new();
    while places.len() < count {
        places.insert((rng.next_u64() % rules.len() as u64) as usize);
    }
    places.into_iter().map(|place| rules[place].clone()).collect()
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

/// A chip for how many rules a sample is of, by its place in [`SIZES`].
#[derive(Component, Default, Clone, Copy)]
struct SampleSize(usize);

pub struct SamplerPlugin;

impl Plugin for SamplerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Sampler>().add_systems(
            Update,
            (keep_counted, hear_counts, show.run_if(resource_changed::<Sampler>.or_else(counting)))
                .chain()
                .in_set(SimSystems::Present),
        );
    }
}

/// The properties asked for, as a card that unfolds: a line that says in a word what is asked
/// for and opens and closes the rest, and under it the chips, group by group.
pub(crate) fn properties_card() -> impl Scene {
    let chips = |range: std::ops::Range<usize>| range.map(chip_scene).collect::<Vec<_>>();
    let (turns, mirrors, keeps) = (chips(TURNS), chips(MIRRORS), chips(KEEPS));
    // The chip of the sparse rules goes with its steps.
    let table = chips(TABLE.start..SPARSE);
    let (same, back_turns, back_mirrors) = (chip_scene(REVERSAL_SAME), chips(REVERSAL_TURNS), chips(REVERSAL_MIRRORS));
    let complemented = chip_scene(REVERSAL_COMPLEMENTED);
    bsn! {
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(8),
            padding: px(8),
            border_radius: px(5),
            flex_shrink: 0.0,
        }
        BackgroundColor(palette::GRAY_2)
        Children [
            (
                // The line opens and closes the chips.
                #LibraryProperties
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(6),
                }
                Hovered
                EntityCursor::System(SystemCursorIcon::Pointer)
                on(|_: On<Pointer<Click>>, mut sampler: ResMut<Sampler>| sampler.open = !sampler.open)
                Children [
                    (
                        icons::icon(icons::OPENS, 11.0, palette::LIGHT_GRAY_2)
                        UiTransform
                        Chevron
                        template_value(Pickable::IGNORE)
                    ),
                    (heading("RULE PROPERTIES") Node { flex_shrink: 0.0 } template_value(Pickable::IGNORE)),
                    (
                        #LibraryFamily
                        caption("")
                        FamilyName
                        Node { flex_grow: 1.0, flex_basis: px(0), min_width: px(0) }
                        template_value(Pickable::IGNORE)
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
                    tile_label("TIME REVERSAL"),
                    (row() Children [ same, complemented ]),
                    (row() Children [ label("turned"), { back_turns } ]),
                    (row() Children [ label("mirrored"), { back_mirrors } ]),
                    tile_label("THE TABLE"),
                    (
                        row()
                        Children [
                            { table },
                            (
                                // The chip and its steps go on to the next line together.
                                Node {
                                    flex_direction: FlexDirection::Row,
                                    align_items: AlignItems::Center,
                                    column_gap: px(5),
                                }
                                Children [
                                    chip_scene(SPARSE),
                                    step("SparseLess", icons::LESS, -1),
                                    step("SparseMore", icons::MORE, 1),
                                ]
                            ),
                        ]
                    ),
                ]
            ),
        ]
    }
}

/// The controls of a sample, on one line: whether its rules are in canonical form, the button
/// that draws it again, and how many rules it is of; and under them how many rules have all
/// that is asked for.
pub(crate) fn generate_controls() -> impl Scene {
    let sizes: Vec<_> = (0..SIZES.len()).map(size_chip).collect();
    bsn! {
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(6),
            flex_shrink: 0.0,
        }
        Children [
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::SpaceBetween,
                    column_gap: px(8),
                }
                Children [
                    (
                        checkbox("Canonical", "GenerateCanonical", Aspect::Rule, "")
                        Node { flex_shrink: 0.0 }
                        CanonicalBox
                        on(|change: On<ValueChange<bool>>, mut sampler: ResMut<Sampler>| {
                            sampler.set_canonical(change.value);
                        })
                    ),
                    (
                        Node {
                            flex_direction: FlexDirection::Row,
                            align_items: AlignItems::Center,
                            column_gap: px(5),
                        }
                        Children [
                            (
                                #Generate
                                button("Generate")
                                Node { margin: UiRect { right: px(3) } }
                                on(generate)
                            ),
                            { sizes },
                        ]
                    ),
                ]
            ),
            // A count of trillions breaks between its numbers, not within one.
            (#GenerateCount caption("") Counted),
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

/// A word in front of a line of chips, as wide as the longest, so that the lines' chips are
/// in a column.
fn label(text: &'static str) -> impl Scene {
    bsn! {
        caption(text)
        Node { width: px(58), margin: UiRect { left: px(2) }, flex_shrink: 0.0 }
    }
}

fn chip_scene(index: usize) -> impl Scene {
    let name = Name::new(format!("Has:{}", CHIPS[index].name));
    let want = Want(index);
    bsn! {
        cas_ui::chip_marked(CHIPS[index].sign, CHIPS[index].label, Aspect::Rule, WantSign(index))
        template_value(name)
        template_value(want)
        on(|click: On<Pointer<Click>>, chips: Query<&Want>, mut sampler: ResMut<Sampler>| {
            if let Ok(&Want(index)) = chips.get(click.entity) {
                sampler.want(index);
            }
        })
    }
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
                    sampler.changed();
                }
            }
        })
        Children [(
            icons::icon(sign, 12.0, palette::LIGHT_GRAY_1)
            template_value(Pickable::IGNORE)
        )]
    }
}

/// A chip for how many rules a sample is of: another number, another sample.
fn size_chip(index: usize) -> impl Scene {
    let (_, written) = SIZES[index];
    let name = Name::new(format!("Generate{written}"));
    let size = SampleSize(index);
    bsn! {
        cas_ui::chip(Sign::Written(written), "", Aspect::Rule)
        template_value(name)
        template_value(size)
        on(|click: On<Pointer<Click>>, chips: Query<&SampleSize>, mut sampler: ResMut<Sampler>| {
            if let Ok(&SampleSize(index)) = chips.get(click.entity)
                && sampler.size != index
            {
                sampler.size = index;
                sampler.changed();
            }
        })
    }
}

/// Draws a sample again.
fn generate(_: On<Activate>, mut sampler: ResMut<Sampler>, mut rng: ResMut<Rng>) {
    sampler.generate(&mut rng);
}

/// The family by the names of its properties.
fn named(family: &Family) -> String {
    let names: Vec<String> = family.constraints().iter().map(|constraint| constraint.to_string()).collect();
    names.join(" + ")
}

/// Whether a count is on its way, which the panel says how far it has got with.
fn counting(sampler: Res<Sampler>) -> bool {
    matches!(sampler.count, Count::Counting(_))
}

/// While the sample is what the panel shows, the family asked for is counted as soon as it
/// is another, and a sample is drawn as soon as there is none for it: when the tab is first
/// opened, and whenever what is asked for changes. One drawn while the family was being
/// counted is drawn again, evenly, once the family is counted and kept.
fn keep_counted(library: Res<RuleLibrary>, mut sampler: ResMut<Sampler>, mut rng: ResMut<Rng>) {
    if !library.generating() {
        return;
    }
    if sampler.known().is_none() {
        sampler.ask();
    }
    if sampler.wants_sample() {
        sampler.generate(&mut rng);
    }
}

/// Takes the counts as they come in.
fn hear_counts(mut sampler: ResMut<Sampler>) {
    if sampler.bypass_change_detection().hear() {
        sampler.set_changed();
    }
}

/// Keeps the panel showing what is asked for and what is known.
fn show(
    sampler: Res<Sampler>,
    mut body: Single<&mut Node, With<Body>>,
    mut chevron: Single<&mut UiTransform, With<Chevron>>,
    chips: Query<(Entity, &Want, Has<Checked>)>,
    sizes: Query<(Entity, &SampleSize, Has<Checked>)>,
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
    // The mark points down while the chips are unfolded.
    let turned = if sampler.open { Rot2::FRAC_PI_2 } else { Rot2::IDENTITY };
    if chevron.rotation != turned {
        chevron.rotation = turned;
    }
    // A chip that is asked for is on: the kit outlines it and brightens its sign.
    for (chip, &Want(index), on) in &chips {
        check(&mut commands, chip, on, sampler.wanted[index]);
    }
    for (chip, &SampleSize(index), on) in &sizes {
        check(&mut commands, chip, on, sampler.size == index);
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
    let count = match sampler.known() {
        None => "counting…".to_string(),
        Some(Count::Counting(progress)) => match progress.rules() {
            0 => "counting…".to_string(),
            gone => format!("counting… {} rules so far", whole(gone)),
        },
        Some(Count::Many) => format!("more than {} rules", whole(COUNTABLE)),
        Some(Count::Known { size, .. }) if size.rules == 0 => "no rule has all of this".to_string(),
        // In canonical form it is the canonical rules that are drawn from.
        Some(Count::Known { size: Size { canonical: Some(canonical), .. }, .. }) if sampler.canonical => {
            so_many(*canonical, "canonical rule")
        }
        Some(Count::Known { size: Size { rules, canonical: Some(canonical) }, .. }) => {
            format!("{} · {} canonical", so_many(*rules, "rule"), whole(*canonical))
        }
        Some(Count::Known { size: Size { rules, canonical: None }, .. }) => {
            format!("{} · too many to count the canonical ones", so_many(*rules, "rule"))
        }
    };
    counted.set_if_neq(Text(count));
    let (checkbox, checked) = *canonical;
    check(&mut commands, checkbox, checked, sampler.canonical);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn in_order(rules: &[BlockRule]) -> bool {
        rules.windows(2).all(|pair| pair[0].table() < pair[1].table())
    }

    #[test]
    fn a_sample_is_of_different_rules_in_the_order_of_their_tables() {
        let mut sampler = Sampler::default();
        let mut rng = Rng::new(7);
        // Every rule there is: a hundred different ones, in order, and not all there are.
        sampler.size = 2;
        sampler.generate(&mut rng);
        assert_eq!(sampler.sample().len(), 100);
        assert!(in_order(sampler.sample()) && !sampler.whole());
        // A quarter turn, a mirror, the cells kept and the empty world left empty: sixteen
        // rules, counted and kept, which asking for them lets go of the sample.
        for name in ["quarter-turn", "mirror", "conserving", "stable-vacuum"] {
            sampler.want(CHIPS.iter().position(|chip| chip.name == name).unwrap());
        }
        assert!(sampler.sample().is_empty() && sampler.known().is_none() && sampler.wants_sample());
        sampler.ask();
        // Drawn while the family is being counted, the sample is wanted again once it is.
        sampler.generate(&mut rng);
        assert!(sampler.drawn && !sampler.settled);
        while matches!(sampler.count, Count::Counting(_)) {
            std::thread::sleep(std::time::Duration::from_millis(5));
            sampler.hear();
        }
        let family = sampler.family();
        assert!(matches!(sampler.known(), Some(Count::Known { size: Size { rules: 16, .. }, kept: Some(_) })));
        assert!(sampler.wants_sample());
        // All of them in a sample of thirty, in order; ten of them drawn evenly, in order too.
        sampler.size = 1;
        sampler.generate(&mut rng);
        assert_eq!(sampler.sample().len(), 16);
        assert!(sampler.whole() && in_order(sampler.sample()));
        assert!(sampler.sample().iter().all(|rule| family.holds(rule)));
        assert!(sampler.settled && !sampler.wants_sample());
        sampler.size = 0;
        sampler.generate(&mut rng);
        assert_eq!(sampler.sample().len(), 10);
        assert!(!sampler.whole() && in_order(sampler.sample()));
        assert!(sampler.sample().iter().all(|rule| family.holds(rule)));
        // In canonical form a sample is of worlds, each once: here every rule is its own.
        sampler.set_canonical(true);
        assert!(sampler.sample().is_empty());
        sampler.size = 2;
        sampler.generate(&mut rng);
        assert_eq!(sampler.sample().len(), 16);
        assert!(sampler.whole() && sampler.sample().iter().all(|rule| *rule == rule.canonical()));
        // Before the count is in, tables are filled in at random: different ones still, in
        // order, with all that is asked for, and not known to be all there are.
        sampler.counted = Family::default();
        sampler.generate(&mut rng);
        let sample = sampler.sample();
        assert!(sample.len() > 1 && sample.len() <= 16 && in_order(sample) && !sampler.whole());
        assert!(sample.iter().all(|rule| family.holds(rule) && *rule == rule.canonical()));
        assert!(!sampler.settled);
        // Every rule there is, as a family too large to keep: a sample is as good as it gets.
        for name in ["quarter-turn", "mirror", "conserving", "stable-vacuum"] {
            sampler.want(CHIPS.iter().position(|chip| chip.name == name).unwrap());
        }
        sampler.ask();
        sampler.generate(&mut rng);
        assert!(sampler.settled && !sampler.wants_sample() && sampler.sample().len() == 100);
    }

    #[test]
    fn the_number_of_cells_or_a_weight_in_its_place() {
        let mut sampler = Sampler::default();
        let (cells, weight) = (EITHER[0], EITHER[1]);
        sampler.want(cells);
        sampler.want(weight);
        assert_eq!((sampler.wanted[cells], sampler.wanted[weight]), (false, true));
        assert_eq!(named(&sampler.family()), "weighted");
        sampler.want(cells);
        assert_eq!(named(&sampler.family()), "conserving");
        sampler.want(cells);
        assert!(sampler.family().constraints().is_empty() && sampler.known().is_some());
    }

    #[test]
    fn the_rule_run_backwards_is_one_property_however_it_is_seen() {
        let chip = |name: &str| CHIPS.iter().position(|chip| chip.name == name).unwrap();
        let mut sampler = Sampler::default();
        // As itself: an involution. As itself mirrored instead; complemented as well; then
        // complemented alone, which is as the same complemented.
        sampler.want(REVERSAL_SAME);
        assert_eq!(named(&sampler.family()), "involution");
        sampler.want(chip("inverse=mirror"));
        assert!(!sampler.wanted[REVERSAL_SAME]);
        assert_eq!(named(&sampler.family()), "inverse=mirror");
        sampler.want(REVERSAL_COMPLEMENTED);
        assert_eq!(named(&sampler.family()), "inverse=mirror,complemented");
        sampler.want(chip("inverse=mirror"));
        assert_eq!(named(&sampler.family()), "inverse=complemented");
        sampler.want(REVERSAL_SAME);
        assert_eq!(named(&sampler.family()), "inverse=complemented");
        sampler.want(REVERSAL_COMPLEMENTED);
        assert_eq!(named(&sampler.family()), "involution");
        // With the others, last.
        sampler.want(chip("quarter-turn"));
        assert_eq!(named(&sampler.family()), "quarter-turn + involution");
        assert_eq!(CHIPS.len(), REVERSAL.end);
    }
}
