//! The rule editor: a second panel showing the rule as its sixteen cases, each a 2×2 block
//! before and after one step.
//!
//! A reversible rule is a permutation of the sixteen blocks, so the editor never lets the table
//! leave that set: the only edit is exchanging the outcomes of two cases (click one outcome,
//! then another). Every permutation is a product of such swaps, and since each intermediate
//! table is a valid rule, edits apply immediately, even while the simulation runs.
//!
//! Under the cases the panel shows what analysis says about the rule, each finding with a
//! picture of it, and the rule as text (its table, and Morita's number if it has one), which
//! can be edited, copied and pasted.

use bevy::{
    clipboard::Clipboard,
    feathers::{constants::fonts, controls::FeathersTextInput, cursor::EntityCursor, palette},
    input_focus::InputFocus,
    picking::hover::Hovered,
    prelude::*,
    text::{EditableText, FontSource, LineBreak, TextEdit, TextEditChange},
    ui_widgets::Activate,
    window::SystemCursorIcon,
};

use cas_core::{
    families::Constraint,
    rules::{BlockRule, Population, Reversed, Symmetry, TURNS_AND_MIRRORS, popcount},
    universe::Universe,
};
use cas_ui::{
    ALIVE, AXES, Aspect, DEAD, GLYPH, ORBIT, Scrolls, button, caption, field_frame, icons, mono, panel_header,
    panel_title, sans, scrolling, section, side_panel, tile, tile_label as label, tile_picture, tile_value as value,
};

use crate::{
    library::RuleLibrary,
    sim::{SimSystems, rule_changed},
};

pub const EDITOR_WIDTH: f32 = 376.0;

/// Side of one cell in the little block pictures.
const CELL: f32 = 12.0;
/// Side of the square a property's picture is drawn in, and of a cell of the vacuum's tiles.
const VACUUM_CELL: f32 = 6.0;

/// The cases, one rotation orbit per row: the blocks in a row are quarter turns of each other.
const ORBITS: [&[u8]; 6] = [&[0], &[1, 2, 8, 4], &[3, 10, 12, 5], &[6, 9], &[7, 11, 14, 13], &[15]];

#[derive(Resource, Default)]
pub struct RuleEditor {
    open: bool,
    /// The case whose outcome is waiting for a partner to swap with.
    selected: Option<u8>,
    /// What the last button or swap did, and the rule it was about: it is dropped as soon as
    /// the rule moves on.
    note: Option<(String, BlockRule)>,
    /// Why the text being typed in the rule field is not a rule.
    typing_error: Option<String>,
}

impl RuleEditor {
    fn say(&mut self, message: impl Into<String>, rule: &BlockRule) {
        self.note = Some((message.into(), rule.clone()));
    }

    pub fn toggle(&mut self) {
        self.open = !self.open;
        self.selected = None;
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn close(&mut self) {
        if self.open {
            self.toggle();
        }
    }
}

/// The panel.
#[derive(Component, Default, Clone)]
struct EditorPanel;

/// One case; the value is the block before the step.
#[derive(Component, Default, Clone, Copy)]
struct CaseCard(u8);

/// The clickable "after" picture of a case.
#[derive(Component, Default, Clone, Copy)]
struct Outcome(u8);

/// One cell of a block picture: bit `bit` of case `input`, before or after the step.
#[derive(Component, Default, Clone, Copy)]
struct BlockCell {
    input: u8,
    bit: u8,
    outcome: bool,
}

/// The line under the rule string: what the last action did, or why the text is no rule.
#[derive(Component, Default, Clone)]
struct Status;

/// A text that says something about the rule.
#[derive(Component, Default, Clone, Copy, PartialEq, Eq, Debug)]
enum Finding {
    #[default]
    Symmetry,
    /// Whether dead and alive are interchangeable.
    States,
    CellCount,
    /// The weight of one corner of a block, for a rule that keeps a weighted count.
    Weight(u8),
    /// How the rule run backwards relates to the rule, in words and as a formula.
    Reversed,
    ReversedFormula,
    Vacuum,
    /// How many of the sixteen blocks the rule changes, and whether only by turning them.
    Blocks,
    /// Whether patterns keep their momentum, the parity of their cells, and superpose.
    Momentum,
    Parity,
    Linear,
    /// The table in hex, Morita's number, and a word about them.
    Hex,
    Espca,
    EspcaNote,
}

/// A piece of a picture whose colour says something about the rule.
#[derive(Component, Clone, Copy)]
enum Lamp {
    /// One place of [`ORBIT`]: lit if the rule is symmetric under that turn or mirror.
    Orbit(usize),
    /// How many of the blocks with `before` cells get `after` cells.
    Flow { before: usize, after: usize },
    /// A cell of the vacuum's tile in one generation of its cycle.
    Vacuum { generation: usize, bit: u8 },
    /// A block, in the small picture of the cases: lit if the rule changes it.
    Changed(u8),
    /// The sign of a property: bright if the rule has it.
    Has(Constraint),
}

/// A piece of a picture that is only there for some rules.
#[derive(Component, Clone, Copy)]
enum Part {
    /// The axis of a mirror, by its place in [`ORBIT`].
    Axis(usize),
    /// The vacuum's tile in one generation of its cycle.
    VacuumTile(usize),
    /// The flow of cell counts, shown unless the rule keeps a weighted count; then the block
    /// of weights is.
    Flow,
    Weights,
    /// Morita's number, which only a rule that looks the same after a quarter turn has.
    Espca,
}

// `bsn!` builds a component from its default.
impl Default for Lamp {
    fn default() -> Self {
        Lamp::Orbit(0)
    }
}

impl Default for Part {
    fn default() -> Self {
        Part::Axis(0)
    }
}

/// The text field holding the rule string.
#[derive(Component, Default, Clone)]
struct RuleStringInput;

pub struct EditorPlugin;

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        let outcome_hovered = |outcomes: Query<(), (With<Outcome>, Changed<Hovered>)>| !outcomes.is_empty();
        app.init_resource::<RuleEditor>().add_observer(use_monospace).add_systems(
            Update,
            (
                follow_rule.run_if(rule_changed),
                sync_findings.run_if(rule_changed),
                sync_editor.run_if(rule_changed.or_eager(resource_changed::<RuleEditor>).or_eager(outcome_hovered)),
                sync_rule_string,
            )
                .chain()
                .in_set(SimSystems::Present),
        );
    }
}

/// A few sentences for the main panel: the preset's description, or what analysis says about
/// a custom rule.
pub fn describe(rule: &BlockRule) -> String {
    if let Some(preset) = rule.preset() {
        return preset.blurb.to_string();
    }
    // Morita's number, for the rules that have one.
    let number = rule.espca().map(|number| format!(", ESPCA-{number}")).unwrap_or_default();
    format!(
        "A custom rule{number}. Symmetry: {}. Dead and alive: {}. Cell count: {}. Time reversal: {}. \
         Vacuum: {}.",
        symmetry(rule),
        states(rule),
        population(rule),
        reversed(rule),
        vacuum_words(rule),
    )
}

fn states(rule: &BlockRule) -> &'static str {
    if rule.is_complement_symmetric() { "interchangeable" } else { "not interchangeable" }
}

fn population(rule: &BlockRule) -> String {
    match rule.population() {
        Population::Conserved => "conserved".to_string(),
        Population::ConservedRelativeToVacuum => "conserved relative to the vacuum".to_string(),
        Population::Weighted(weights) => format!("conserved by weight {}", Population::weights_text(&weights)),
        Population::NotConserved => "not conserved".to_string(),
    }
}

fn symmetry(rule: &BlockRule) -> &'static str {
    match rule.symmetry() {
        Symmetry::Full => "all rotations and mirrors",
        Symmetry::Rotations => "rotations only",
        Symmetry::HalfTurnAndAxisMirrors => "half turn and axis mirrors",
        Symmetry::HalfTurnAndDiagonalMirrors => "half turn and diagonal mirrors",
        Symmetry::HalfTurn => "half turn only",
        Symmetry::LeftRightMirror => "left-right mirror only",
        Symmetry::TopBottomMirror => "top-bottom mirror only",
        Symmetry::DiagonalMirror => "one diagonal mirror only",
        Symmetry::None => "none",
    }
}

fn vacuum_words(rule: &BlockRule) -> String {
    match rule.vacuum_cycle().len() {
        1 => "stable".to_string(),
        period => format!("repeats every {period} generations"),
    }
}

/// How many of the sixteen blocks the rule changes, and whether it only turns them.
fn blocks(rule: &BlockRule) -> String {
    let changed = (0..16).filter(|&block| rule.table()[block] != block as u8).count();
    match (changed, Constraint::Turning.holds(rule)) {
        (0, _) => "none changes".to_string(),
        // (A mirror image of a block is always a turn of it as well.)
        (_, true) => format!("{changed} of 16 change, each turned"),
        (_, false) => format!("{changed} of 16 change"),
    }
}

fn kept(kept: bool) -> &'static str {
    if kept { "kept" } else { "not kept" }
}

/// The relation of the rule run backwards to the rule, in icons: the same, a mirror image,
/// the two states exchanged, both, or none of it.
fn reversed_formula(rule: &BlockRule) -> String {
    match rule.reversed() {
        Reversed::SameRule => icons::EQUAL.to_string(),
        Reversed::Transformed => icons::MIRROR.to_string(),
        Reversed::Complemented => icons::STATES.to_string(),
        Reversed::TransformedAndComplemented => format!("{}{}", icons::MIRROR, icons::STATES),
        Reversed::DifferentRule => icons::UNEQUAL.to_string(),
    }
}

fn reversed(rule: &BlockRule) -> &'static str {
    match rule.reversed() {
        Reversed::SameRule => "the same rule",
        Reversed::Transformed => "the rule turned or mirrored",
        Reversed::Complemented => "the rule complemented",
        Reversed::TransformedAndComplemented => "the rule mirrored and complemented",
        Reversed::DifferentRule => "a different rule",
    }
}

pub fn editor_panel() -> impl Scene {
    let orbits: Vec<_> = ORBITS.iter().map(|orbit| orbit_row(orbit)).collect();
    bsn! {
        #RuleEditor
        side_panel(EDITOR_WIDTH, bsn_list![
            panel_header(panel_title(Aspect::Rule, "Rule editor"), bsn! {
                #EditorClose
                on(|_: On<Activate>, mut editor: ResMut<RuleEditor>| editor.toggle())
            }),
            scrolling(Scrolls::Body, bsn! {
                #EditorBody
                Children [
                    { editor_body(orbits) },
                ]
            }),
        ])
        EditorPanel
    }
}

/// The cases, the buttons that replace the table, what analysis says, and the rule as text.
fn editor_body(orbits: Vec<impl Scene>) -> impl SceneList {
    bsn_list![
            caption("Each of the 16 blocks before → after one step. To edit the rule, swap two outcomes: click one, then the other."),
            (
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(4),
                }
                Children [ { orbits } ]
            ),
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(6),
                }
                Children [
                    (
                        #RuleIdentity
                        button("Identity")
                        Node { flex_grow: 1.0 }
                        on(|_: On<Activate>,
                            mut universe: ResMut<Universe>,
                            mut editor: ResMut<RuleEditor>| {
                            universe.set_rule(BlockRule::identity());
                            editor.say("The identity: nothing ever changes.", universe.rule());
                        })
                    ),
                    (
                        #RuleInverse
                        button("Inverse")
                        Node { flex_grow: 1.0 }
                        on(|_: On<Activate>,
                            mut universe: ResMut<Universe>,
                            mut editor: ResMut<RuleEditor>| {
                            let inverse = universe.rule().inverted();
                            universe.set_rule(inverse);
                            editor.say("The inverse table: every block goes back to where it came from.", universe.rule());
                        })
                    ),
                    (
                        // The canonical form: the least table among the rule's turns and
                        // mirrors, the generations of its vacuum's cycle it could begin at,
                        // and the vacuum's flickering away.
                        #RuleCanonical
                        button("Canonical")
                        Node { flex_grow: 1.0 }
                        on(|_: On<Activate>,
                            mut universe: ResMut<Universe>,
                            mut editor: ResMut<RuleEditor>| {
                            let canonical = universe.rule().canonical();
                            if canonical == *universe.rule() {
                                editor.say("This table is already canonical: it comes first, in order, of its turns and mirrors and of the same rule begun at any other generation of the vacuum's cycle.", universe.rule());
                            } else {
                                universe.set_rule(canonical);
                                editor.say("The same world in canonical form: the first table, in order, of its turns and mirrors and of the same rule begun at any other generation of the vacuum's cycle.", universe.rule());
                            }
                        })
                    ),
                ]
            ),
            findings(),
            section("RULE STRING", bsn_list![
                (
                    field_frame()
                    Children [(
                        #RuleString
                        @FeathersTextInput {
                            @max_characters: 96usize,
                        }
                        RuleStringInput
                        on(rule_string_edited)
                    )]
                ),
                (
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: px(2),
                        padding: UiRect { left: px(6) },
                    }
                    Children [
                        (
                            // The table in hex and Morita's number on one line, whatever
                            // the room; the word on them goes below.
                            Node {
                                flex_direction: FlexDirection::Row,
                                column_gap: px(8),
                            }
                            Children [
                                (
                                    #RuleHex
                                    mono("", 12.0, palette::LIGHT_GRAY_1)
                                    Node { flex_shrink: 0.0 }
                                    TextLayout { linebreak: LineBreak::NoWrap }
                                    template_value(Finding::Hex)
                                ),
                                (
                                    // Not there without a number: an empty text would still
                                    // have the row's gap on either side of it.
                                    #RuleEspca
                                    mono("", 12.0, palette::LIGHT_GRAY_1)
                                    Node { flex_shrink: 0.0 }
                                    TextLayout { linebreak: LineBreak::NoWrap }
                                    template_value(Finding::Espca)
                                    template_value(Part::Espca)
                                ),
                            ]
                        ),
                        (caption("") template_value(Finding::EspcaNote)),
                    ]
                ),
                (
                    Node {
                        flex_direction: FlexDirection::Row,
                        column_gap: px(6),
                    }
                    Children [
                        (
                            #RuleCopy
                            button("Copy")
                            Node { flex_grow: 1.0 }
                            on(copy_rule)
                        ),
                        (
                            #RulePaste
                            button("Paste")
                            Node { flex_grow: 1.0 }
                            on(paste_rule)
                        ),
                        (
                            // Into the library, which opens to give the rule its name.
                            #RuleKeep
                            button("Keep")
                            Node { flex_grow: 1.0 }
                            on(|_: On<Activate>, universe: Res<Universe>, mut library: ResMut<RuleLibrary>| {
                                library.keep(universe.rule());
                            })
                        ),
                    ]
                ),
                caption("Outcomes of blocks 0 to 15 (cells count 1, 2, 4, 8 from the top-left), a preset's name, or espca-01c5ef."),
                (#EditorNote caption("") Status),
            ]),
    ]
}

/// What analysis says about the rule: each finding in words, next to a picture of it.
fn findings() -> impl Scene {
    // An icon or two that say how the rule run backwards relates to the rule.
    let formula = |finding: Finding, color: Color| bsn_list![(icons::icon("", 20.0, color) template_value(finding))];
    let tiles: Vec<_> = (0..16).map(vacuum_tile).collect();
    section(
        "PROPERTIES",
        bsn_list![
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(6),
                }
                Children [
                    finding("SYMMETRY", Finding::Symmetry, symmetry_picture()),
                    finding("DEAD AND ALIVE", Finding::States, sign(icons::STATES, Constraint::Complement)),
                ]
            ),
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(6),
                }
                Children [
                    finding("CELL COUNT", Finding::CellCount, flow_picture()),
                    finding("TIME REVERSAL", Finding::Reversed, formula(Finding::ReversedFormula, palette::LIGHT_GRAY_1)),
                ]
            ),
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(6),
                }
                Children [
                    finding("BLOCKS", Finding::Blocks, blocks_picture()),
                    finding("MOMENTUM", Finding::Momentum, sign(icons::MOMENTUM, Constraint::Momentum)),
                ]
            ),
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(6),
                }
                Children [
                    finding("PARITY", Finding::Parity, sign(icons::PARITY, Constraint::Parity)),
                    finding("SUPERPOSITION", Finding::Linear, sign(icons::LINEAR, Constraint::Linear)),
                ]
            ),
            (
                // The empty world through the generations of its cycle.
                tile()
                Children [
                    (
                        Node {
                            width: px(GLYPH),
                            flex_shrink: 0.0,
                        }
                        Children [ label("VACUUM") ]
                    ),
                    (
                        Node {
                            flex_direction: FlexDirection::Row,
                            flex_wrap: FlexWrap::Wrap,
                            column_gap: px(3),
                            row_gap: px(3),
                            flex_shrink: 0.0,
                            // Eight to a row: the longest cycle takes two.
                            max_width: px(8.0 * (2.0 * VACUUM_CELL + 6.0) - 3.0),
                        }
                        Children [ { tiles } ]
                    ),
                    (
                        #FindingVacuum
                        value("") template_value(Finding::Vacuum)
                        Node { flex_grow: 1.0, flex_basis: px(0) }
                    ),
                ]
            ),
        ],
    )
}

/// A finding: its picture, its name and what was found.
fn finding(name: &'static str, finding: Finding, picture: impl SceneList) -> impl Scene {
    // The rig finds what was found by this name.
    let named = Name::new(format!("Finding{finding:?}"));
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
                    flex_grow: 1.0,
                    flex_basis: px(0),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(2),
                }
                Children [
                    label(name),
                    (value("") template_value(finding) template_value(named)),
                ]
            ),
        ]
    }
}

/// The symmetry of the rule as a picture: a point and its images under the turns and mirrors
/// that leave the rule as it is, with the axes of those mirrors.
fn symmetry_picture() -> impl SceneList {
    let middle = GLYPH / 2.0;
    let dots: Vec<_> = (0..ORBIT.len())
        .map(|element| {
            let (x, y) = ORBIT[element];
            let lamp = Lamp::Orbit(element);
            bsn! {
                Node {
                    position_type: PositionType::Absolute,
                    left: px(middle + x - 2.5),
                    top: px(middle + y - 2.5),
                    width: px(5),
                    height: px(5),
                    border_radius: BorderRadius::MAX,
                }
                BackgroundColor(palette::GRAY_3)
                template_value(lamp)
            }
        })
        .collect();
    let color = Aspect::Rule.color();
    let axes: Vec<_> = AXES
        .into_iter()
        .map(|(element, degrees): (usize, f32)| {
            let part = Part::Axis(element);
            let turned = UiTransform::from_rotation(Rot2::degrees(degrees));
            bsn! {
                Node {
                    display: Display::None,
                    position_type: PositionType::Absolute,
                    left: px(2),
                    top: px(middle - 0.5),
                    width: px(GLYPH - 4.0),
                    height: px(1),
                }
                BackgroundColor(color)
                template_value(turned)
                template_value(part)
            }
        })
        .collect();
    bsn_list![{ axes }, { dots }]
}

/// The cases above in small, a square for each block: lit where the rule changes it.
fn blocks_picture() -> impl SceneList {
    const SIDE: f32 = 6.0;
    const STEP: f32 = 7.5;
    let squares: Vec<_> = ORBITS
        .iter()
        .enumerate()
        .flat_map(|(row, orbit)| {
            let left = (GLYPH - (orbit.len() as f32 * STEP - (STEP - SIDE))) / 2.0;
            orbit.iter().enumerate().map(move |(place, &block)| {
                let lamp = Lamp::Changed(block);
                bsn! {
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(left + STEP * place as f32),
                        top: px(2.0 + STEP * row as f32),
                        width: px(SIDE),
                        height: px(SIDE),
                        border_radius: px(1),
                    }
                    BackgroundColor(palette::GRAY_3)
                    template_value(lamp)
                }
            })
        })
        .collect();
    bsn_list![{ squares }]
}

/// The icon a property goes by, bright when the rule has it.
fn sign(sign: &'static str, property: Constraint) -> impl SceneList {
    let lamp = Lamp::Has(property);
    bsn_list![(icons::icon(sign, 26.0, palette::GRAY_3) template_value(lamp))]
}

/// Where the blocks go, by their number of cells: before across, after upwards. A rule that
/// conserves the cells of a pattern lights the diagonal.
fn flow_picture() -> impl SceneList {
    let squares: Vec<_> = (0..25)
        .map(|i| {
            let (before, after) = (i % 5, i / 5);
            let lamp = Lamp::Flow { before, after };
            bsn! {
                Node {
                    position_type: PositionType::Absolute,
                    left: px(2.0 + 9.0 * before as f32),
                    bottom: px(2.0 + 9.0 * after as f32),
                    width: px(8),
                    height: px(8),
                    border_radius: px(1),
                }
                BackgroundColor(Color::NONE)
                template_value(lamp)
            }
        })
        .collect();
    // The weights of the four corners, written in a block: the top row, then the bottom.
    let weights: Vec<_> = (0..4u8)
        .map(|corner| {
            let finding = Finding::Weight(corner);
            bsn! {
                Node {
                    position_type: PositionType::Absolute,
                    left: px(3.0 + 21.0 * (corner & 1) as f32),
                    top: px(3.0 + 21.0 * (corner >> 1) as f32),
                    width: px(20),
                    height: px(20),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border_radius: px(2),
                }
                BackgroundColor(palette::GRAY_2)
                Children [ (mono("", 13.0, ALIVE) template_value(finding)) ]
            }
        })
        .collect();
    let (flow, weighed) = (Part::Flow, Part::Weights);
    bsn_list![
        (
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                top: px(0),
                width: px(GLYPH),
                height: px(GLYPH),
            }
            template_value(flow)
            Children [ { squares } ]
        ),
        (
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                top: px(0),
                width: px(GLYPH),
                height: px(GLYPH),
                display: Display::None,
            }
            template_value(weighed)
            Children [ { weights } ]
        ),
    ]
}

/// The vacuum in one generation of its cycle, as a small block.
fn vacuum_tile(generation: usize) -> impl Scene {
    let part = Part::VacuumTile(generation);
    let cells: Vec<_> = (0..4u8)
        .map(|bit| {
            let lamp = Lamp::Vacuum { generation, bit };
            bsn! {
                Node {
                    position_type: PositionType::Absolute,
                    left: px(1.0 + (VACUUM_CELL + 1.0) * (bit & 1) as f32),
                    top: px(1.0 + (VACUUM_CELL + 1.0) * (bit >> 1) as f32),
                    width: px(VACUUM_CELL),
                    height: px(VACUUM_CELL),
                }
                BackgroundColor(DEAD)
                template_value(lamp)
            }
        })
        .collect();
    bsn! {
        Node {
            display: Display::None,
            width: px(2.0 * VACUUM_CELL + 3.0),
            height: px(2.0 * VACUUM_CELL + 3.0),
            border_radius: px(2),
        }
        BackgroundColor(palette::GRAY_3)
        template_value(part)
        Children [ { cells } ]
    }
}

/// `[before][after]`: of the blocks with `before` cells, the share that gets `after` cells.
/// The rule is taken relative to its vacuum, over the whole of the vacuum's cycle: that is
/// what happens to the cells of a pattern.
fn flow(rule: &BlockRule) -> [[f32; 5]; 5] {
    let tables = rule.relative_to_vacuum();
    let mut flow = [[0.0; 5]; 5];
    for table in &tables {
        for block in 0..16u8 {
            let (before, after) = (popcount(block), popcount(table.table()[block as usize]));
            flow[before as usize][after as usize] += 1.0;
        }
    }
    for (before, shares) in flow.iter_mut().enumerate() {
        // So many blocks have that many cells.
        let blocks = [1.0, 4.0, 6.0, 4.0, 1.0][before] * tables.len() as f32;
        shares.iter_mut().for_each(|share| *share /= blocks);
    }
    flow
}

/// Which ways of turning and mirroring leave the rule as it is, in the order of [`ORBIT`].
fn symmetries(rule: &BlockRule) -> [bool; 8] {
    std::array::from_fn(|element| match element {
        0 => true,
        _ => rule.commutes_with(TURNS_AND_MIRRORS[element - 1]),
    })
}

/// Shows what analysis says about a new rule.
fn sync_findings(
    universe: Res<Universe>,
    mut lamps: Query<(&Lamp, &mut BackgroundColor)>,
    mut signs: Query<(&Lamp, &mut TextColor)>,
    mut parts: Query<(&Part, &mut Node)>,
    mut texts: Query<(&Finding, &mut Text)>,
) {
    let rule = universe.rule();
    let (symmetries, flow, vacuum) = (symmetries(rule), flow(rule), rule.vacuum_cycle());
    for (lamp, mut color) in &mut lamps {
        color.0 = match *lamp {
            Lamp::Orbit(element) if symmetries[element] => palette::WHITE,
            Lamp::Orbit(_) => palette::GRAY_3,
            Lamp::Flow { before, after } => match flow[before][after] {
                0.0 => Color::NONE,
                share => ALIVE.with_alpha(0.3 + 0.7 * share),
            },
            Lamp::Vacuum { generation, bit } => {
                let alive = vacuum.get(generation).is_some_and(|tile| tile >> bit & 1 == 1);
                if alive { ALIVE } else { DEAD }
            }
            Lamp::Changed(block) if rule.table()[block as usize] != block => Aspect::Rule.color(),
            Lamp::Changed(_) => palette::GRAY_3,
            Lamp::Has(_) => continue,
        };
    }
    for (lamp, mut color) in &mut signs {
        if let Lamp::Has(property) = *lamp {
            color.0 = if property.holds(rule) { ALIVE } else { palette::GRAY_3 };
        }
    }
    let weights = match rule.population() {
        Population::Weighted(weights) => Some(weights),
        _ => None,
    };
    for (part, mut node) in &mut parts {
        let shown = match *part {
            Part::Axis(element) => symmetries[element],
            Part::VacuumTile(generation) => generation < vacuum.len(),
            Part::Flow => weights.is_none(),
            Part::Weights => weights.is_some(),
            Part::Espca => rule.espca().is_some(),
        };
        node.display = if shown { Display::Flex } else { Display::None };
    }
    let number = rule.espca();
    for (finding, mut text) in &mut texts {
        let content = match finding {
            Finding::Symmetry => symmetry(rule).to_string(),
            Finding::States => states(rule).to_string(),
            Finding::CellCount => population(rule),
            Finding::Weight(corner) => weights.map_or(String::new(), |weights| weights[*corner as usize].to_string()),
            Finding::Reversed => reversed(rule).to_string(),
            Finding::ReversedFormula => reversed_formula(rule),
            Finding::Vacuum => vacuum_words(rule),
            Finding::Blocks => blocks(rule),
            Finding::Momentum => kept(Constraint::Momentum.holds(rule)).to_string(),
            Finding::Parity => kept(Constraint::Parity.holds(rule)).to_string(),
            Finding::Linear if Constraint::Linear.holds(rule) => "patterns superpose".to_string(),
            Finding::Linear => "patterns do not superpose".to_string(),
            Finding::Hex => rule.hex(),
            Finding::Espca => number.as_ref().map(|number| format!("ESPCA-{number}")).unwrap_or_default(),
            Finding::EspcaNote if number.is_some() => "the table in hex, and Morita's number".to_string(),
            Finding::EspcaNote => "the table in hex; no ESPCA number, the rule changes with a quarter turn".to_string(),
        };
        text.set_if_neq(Text(content));
    }
}

fn orbit_row(orbit: &'static [u8]) -> impl Scene {
    let cards: Vec<_> = orbit.iter().map(|&input| case_card(input)).collect();
    bsn! {
        Node {
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::Center,
            column_gap: px(6),
        }
        Children [ { cards } ]
    }
}

fn case_card(input: u8) -> impl Scene {
    let card = CaseCard(input);
    let outcome = Outcome(input);
    let name = Name::new(format!("Out{input}"));
    bsn! {
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(3),
            padding: px(4),
            border_radius: px(5),
        }
        template_value(card)
        BackgroundColor(Color::NONE)
        Children [
            block(input, false),
            sans("→", 12.0, palette::LIGHT_GRAY_2),
            (
                block(input, true)
                template_value(name)
                template_value(outcome)
                Hovered
                EntityCursor::System(SystemCursorIcon::Pointer)
                on(outcome_clicked)
            ),
        ]
    }
}

/// A 2×2 block picture. Its background shows through the gaps as a frame; the cells and rows
/// are invisible to picking so that a click lands on the block itself.
fn block(input: u8, outcome: bool) -> impl Scene {
    bsn! {
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(1),
            padding: px(1),
            border_radius: px(2),
        }
        BackgroundColor(palette::GRAY_3)
        Children [
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(1),
                }
                template_value(Pickable::IGNORE)
                Children [
                    block_cell(input, 0, outcome),
                    block_cell(input, 1, outcome),
                ]
            ),
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(1),
                }
                template_value(Pickable::IGNORE)
                Children [
                    block_cell(input, 2, outcome),
                    block_cell(input, 3, outcome),
                ]
            ),
        ]
    }
}

fn block_cell(input: u8, bit: u8, outcome: bool) -> impl Scene {
    let cell = BlockCell { input, bit, outcome };
    bsn! {
        Node {
            width: px(CELL),
            height: px(CELL),
        }
        BackgroundColor(DEAD)
        template_value(cell)
        template_value(Pickable::IGNORE)
    }
}

/// Click one outcome, then another: the two cases exchange outcomes.
fn outcome_clicked(
    click: On<Pointer<Click>>,
    outcomes: Query<&Outcome>,
    mut editor: ResMut<RuleEditor>,
    mut universe: ResMut<Universe>,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    let Ok(&Outcome(input)) = outcomes.get(click.entity) else {
        return;
    };
    match editor.selected.take() {
        None => editor.selected = Some(input),
        // Clicking the selected outcome again lets go of it.
        Some(first) if first == input => {}
        Some(first) => {
            let mut rule = universe.rule().clone();
            rule.swap_outcomes(first, input);
            universe.set_rule(rule);
            editor.say(format!("Swapped the outcomes of blocks {first} and {input}."), universe.rule());
        }
    }
}

/// Typing or pasting in the rule field applies the rule as soon as the text is a valid one.
fn rule_string_edited(
    _: On<TextEditChange>,
    field: Single<(Entity, &EditableText), With<RuleStringInput>>,
    focus: Res<InputFocus>,
    mut editor: ResMut<RuleEditor>,
    mut universe: ResMut<Universe>,
) {
    let (entity, text) = *field;
    // The field is also rewritten whenever the rule changes; only the user's edits count.
    if focus.get() != Some(entity) {
        return;
    }
    let typing_error = match text.value().to_string().parse::<BlockRule>() {
        // Moving the caret reports an edit as well, so most of the time nothing is new.
        Ok(rule) if rule == *universe.rule() => None,
        Ok(rule) => {
            universe.set_rule(rule);
            None
        }
        Err(error) => Some(error),
    };
    if editor.typing_error != typing_error {
        editor.typing_error = typing_error;
    }
}

fn copy_rule(
    _: On<Activate>,
    universe: Res<Universe>,
    mut clipboard: ResMut<Clipboard>,
    mut editor: ResMut<RuleEditor>,
) {
    let text = universe.rule().to_string();
    let message = match clipboard.set_text(text.as_str()) {
        Ok(()) => "Copied to the clipboard.".to_string(),
        Err(error) => format!("The clipboard is not available ({error:?})."),
    };
    editor.say(message, universe.rule());
    // Also leave it where it can be picked up without a clipboard.
    info!("rule: {text}");
}

fn paste_rule(
    _: On<Activate>,
    mut clipboard: ResMut<Clipboard>,
    mut universe: ResMut<Universe>,
    mut editor: ResMut<RuleEditor>,
) {
    let message = match clipboard.fetch_text().poll_result() {
        Some(Ok(text)) => match text.parse::<BlockRule>() {
            Ok(rule) => {
                universe.set_rule(rule);
                "Pasted from the clipboard.".to_string()
            }
            Err(error) => error,
        },
        Some(Err(error)) => format!("The clipboard is not available ({error:?})."),
        None => "The clipboard did not answer.".to_string(),
    };
    editor.say(message, universe.rule());
}

/// The rule string reads better in a fixed-width font. The text input's own scene already
/// sets a `TextFont`, which a second one in ours would duplicate, so it is replaced here.
fn use_monospace(add: On<Add, RuleStringInput>, assets: Res<AssetServer>, mut commands: Commands) {
    commands.entity(add.entity).insert(TextFont {
        font: FontSource::Handle(assets.load(fonts::MONO)),
        font_size: FontSize::Px(12.0),
        ..default()
    });
}

/// A new rule: what was about the old one is dropped.
fn follow_rule(universe: Res<Universe>, mut editor: ResMut<RuleEditor>) {
    let rule = universe.rule();
    editor.selected = None;
    if editor.note.as_ref().is_some_and(|(_, about)| about != rule) {
        editor.note = None;
    }
}

/// Redraws the panel when the rule, the editor state or the hovered outcome changed.
fn sync_editor(
    editor: Res<RuleEditor>,
    universe: Res<Universe>,
    mut panel: Single<&mut Node, (With<EditorPanel>, Without<Status>)>,
    mut cells: Query<(&BlockCell, &mut BackgroundColor), (Without<CaseCard>, Without<Outcome>)>,
    mut cards: Query<(&CaseCard, &mut BackgroundColor), (Without<BlockCell>, Without<Outcome>)>,
    mut outcomes: Query<(&Outcome, &Hovered, &mut BackgroundColor), (Without<BlockCell>, Without<CaseCard>)>,
    mut status: Single<(&mut Text, &mut Node), (With<Status>, Without<EditorPanel>)>,
) {
    let table = universe.rule().table();

    let display = if editor.open { Display::Flex } else { Display::None };
    if panel.display != display {
        panel.display = display;
    }
    for (cell, mut color) in &mut cells {
        let block = if cell.outcome { table[cell.input as usize] } else { cell.input };
        color.0 = if block >> cell.bit & 1 == 1 { ALIVE } else { DEAD };
    }
    // Cases the rule actually changes stand out from the ones it leaves alone.
    for (card, mut color) in &mut cards {
        color.0 = if table[card.0 as usize] != card.0 { palette::GRAY_2 } else { Color::NONE };
    }
    for (outcome, hovered, mut frame) in &mut outcomes {
        frame.0 = if editor.selected == Some(outcome.0) {
            Aspect::Rule.color()
        } else if hovered.0 {
            palette::LIGHT_GRAY_2
        } else {
            palette::GRAY_3
        };
    }
    let message = editor.typing_error.clone().or_else(|| editor.note.as_ref().map(|(message, _)| message.clone()));
    // With nothing to say the line is not there at all.
    let (text, line) = &mut *status;
    let display = if message.is_some() { Display::Flex } else { Display::None };
    if line.display != display {
        line.display = display;
    }
    text.set_if_neq(Text(message.unwrap_or_default()));
}

/// Shows the rule in the text field, unless the user is typing in it: on a rule change, and
/// when the field loses the focus with something unfinished in it.
fn sync_rule_string(
    universe: Res<Universe>,
    focus: Res<InputFocus>,
    mut field: Single<(Entity, &mut EditableText), With<RuleStringInput>>,
    mut editor: ResMut<RuleEditor>,
    mut shown: Local<Option<BlockRule>>,
) {
    let (entity, text) = &mut *field;
    if focus.get() == Some(*entity) {
        return;
    }
    if shown.as_ref() == Some(universe.rule()) && !focus.is_changed() {
        return;
    }
    *shown = Some(universe.rule().clone());
    let wanted = universe.rule().to_string();
    if text.value().to_string() != wanted {
        text.queue_edit(TextEdit::SelectAll);
        text.queue_edit(TextEdit::Insert(wanted.into()));
    }
    if editor.typing_error.is_some() {
        editor.typing_error = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orbits_cover_every_case_once() {
        let mut seen: Vec<u8> = ORBITS.iter().flat_map(|orbit| orbit.iter().copied()).collect();
        seen.sort_unstable();
        assert_eq!(seen, (0..16).collect::<Vec<u8>>());
        for orbit in ORBITS {
            for pair in orbit.windows(2) {
                assert_eq!(cas_core::rules::rotate_cw(pair[0]), pair[1]);
            }
        }
    }

    #[test]
    fn rules_are_described() {
        let single_rotation: BlockRule = "single-rotation".parse().unwrap();
        assert!(describe(&single_rotation).starts_with("Blocks with exactly one live cell"));
        assert_eq!(
            describe(&BlockRule::identity()),
            "A custom rule, ESPCA-08cadf. Symmetry: all rotations and mirrors. Dead and alive: \
             interchangeable. Cell count: conserved. Time reversal: the same rule. Vacuum: stable."
        );
        assert_eq!(
            describe(&lopsided()),
            "A custom rule. Symmetry: none. Dead and alive: not interchangeable. Cell count: not \
             conserved. Time reversal: the same rule. Vacuum: stable."
        );
    }

    #[test]
    fn findings_are_what_the_pictures_show() {
        let rule = |name: &str| name.parse::<BlockRule>().unwrap();
        // Single Rotation looks the same after every turn and in no mirror.
        assert_eq!(symmetries(&rule("single-rotation")), [true, true, true, true, false, false, false, false]);
        assert_eq!(symmetries(&rule("critters")), [true; 8]);
        assert_eq!(symmetries(&lopsided()), [true, false, false, false, false, false, false, false]);
        // The places of the orbit are the images of the first under the same turns and mirrors.
        let turned = |(x, y): (f32, f32)| (-y, x);
        assert_eq!(ORBIT[1], turned(ORBIT[0]));
        assert_eq!(ORBIT[2], turned(ORBIT[1]));
        assert_eq!(ORBIT[3], turned(ORBIT[2]));
        assert_eq!(ORBIT[4], (-ORBIT[0].0, ORBIT[0].1));
        assert_eq!(ORBIT[5], (ORBIT[0].0, -ORBIT[0].1));
        assert_eq!(ORBIT[6], (ORBIT[0].1, ORBIT[0].0));
        assert_eq!(ORBIT[7], (-ORBIT[0].1, -ORBIT[0].0));

        // Cells are conserved: every block keeps its count. Critters does so relative to
        // its vacuum, which is what the picture shows.
        let diagonal: [[f32; 5]; 5] =
            std::array::from_fn(|before| std::array::from_fn(|after| (before == after) as u8 as f32));
        assert_eq!(flow(&rule("single-rotation")), diagonal);
        assert_eq!(flow(&rule("critters")), diagonal);
        // In ESPCA-0945df a lone cell becomes two, and four of the six pairs become one.
        let growing = flow(&rule("espca-0945df"));
        assert_eq!((growing[1][2], growing[2][1], growing[2][2]), (1.0, 4.0 / 6.0, 2.0 / 6.0));

        assert_eq!(reversed_formula(&rule("bbm")), icons::EQUAL);
        assert_eq!(reversed_formula(&rule("single-rotation")), icons::MIRROR);
        assert_eq!(reversed_formula(&rule("critters")), icons::STATES);
        assert_eq!((states(&rule("hpp-gas")), states(&rule("critters"))), ("interchangeable", "not interchangeable"));
        assert_eq!(vacuum_words(&rule("critters")), "repeats every 2 generations");
    }

    fn lopsided() -> BlockRule {
        let mut rule = BlockRule::identity();
        rule.swap_outcomes(1, 3);
        rule
    }
}
