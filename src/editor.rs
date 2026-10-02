//! The rule editor: a second panel showing the rule as its sixteen cases, each a 2×2 block
//! before and after one step.
//!
//! A reversible rule is a permutation of the sixteen blocks, so the editor never lets the table
//! leave that set: the only edit is exchanging the outcomes of two cases (click one outcome,
//! then another). Every permutation is a product of such swaps, and since each intermediate
//! table is a valid rule, edits apply immediately, even while the simulation runs.
//!
//! The rule is also shown as text (the table, see `rules.rs`), which can be edited, copied and
//! pasted.

use bevy::{
    clipboard::Clipboard,
    feathers::{
        constants::fonts,
        controls::{FeathersButton, FeathersTextInput, FeathersTextInputContainer},
        cursor::EntityCursor,
        palette,
        theme::ThemedText,
    },
    input_focus::InputFocus,
    picking::hover::Hovered,
    prelude::*,
    text::{EditableText, FontSource, FontSourceTemplate, FontWeight, TextEdit, TextEditChange},
    ui_widgets::Activate,
    window::SystemCursorIcon,
};

use crate::{
    rules::{BlockRule, Population, Reversed, Symmetry},
    sim::{Rng, SimSystems, Universe, rule_changed},
    ui::{Aspect, caption, panel_title, readout, section, side_panel},
    view::{ALIVE, DEAD},
};

pub const EDITOR_WIDTH: f32 = 376.0;

/// Side of one cell in the little block pictures.
const CELL: f32 = 12.0;

/// The cases, one rotation orbit per row: the blocks in a row are quarter turns of each other.
const ORBITS: [&[u8]; 6] = [
    &[0],
    &[1, 2, 8, 4],
    &[3, 10, 12, 5],
    &[6, 9],
    &[7, 11, 14, 13],
    &[15],
];

#[derive(Resource, Default)]
pub struct RuleEditor {
    open: bool,
    /// The case whose outcome is waiting for a partner to swap with.
    selected: Option<u8>,
    /// The last rule that was not a preset; "Custom…" in the rule menu brings it back.
    custom: Option<BlockRule>,
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

    /// Opens the editor on the last hand-made rule (or on the current rule if there is none yet).
    pub fn open_custom(&mut self, universe: &mut Universe) {
        if let Some(rule) = &self.custom {
            universe.set_rule(rule.clone());
        }
        self.open = true;
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

#[derive(Component, Default, Clone, Copy, PartialEq, Eq)]
enum EditorText {
    #[default]
    Properties,
    Status,
}

/// The text field holding the rule string.
#[derive(Component, Default, Clone)]
struct RuleStringInput;

pub struct EditorPlugin;

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        let outcome_hovered =
            |outcomes: Query<(), (With<Outcome>, Changed<Hovered>)>| !outcomes.is_empty();
        app.init_resource::<RuleEditor>()
            .add_observer(use_monospace)
            .add_systems(
                Update,
                (
                    follow_rule.run_if(rule_changed),
                    sync_editor.run_if(
                        rule_changed
                            .or_eager(resource_changed::<RuleEditor>)
                            .or_eager(outcome_hovered),
                    ),
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
        "A custom rule{number}. Population: {}. Symmetry: {}. Vacuum: {}. Reversed: {}.",
        population(rule),
        symmetry(rule),
        vacuum(rule),
        reversed(rule),
    )
}

/// The same analysis as a table, for the editor.
fn properties(rule: &BlockRule) -> String {
    let two_states = if rule.is_complement_symmetric() {
        "interchangeable"
    } else {
        "not interchangeable"
    };
    let number = rule.espca().map(|number| format!("\nespca       {number}")).unwrap_or_default();
    format!(
        "population  {}\n\
         symmetry    {}\n\
         two states  {two_states}\n\
         vacuum      {}\n\
         reversed    {}{number}",
        population(rule),
        symmetry(rule),
        vacuum(rule),
        reversed(rule),
    )
}

fn population(rule: &BlockRule) -> &'static str {
    match rule.population() {
        Population::Conserved => "conserved",
        Population::ConservedRelativeToVacuum => "conserved relative to the vacuum",
        Population::NotConserved => "not conserved",
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

fn vacuum(rule: &BlockRule) -> String {
    match rule.vacuum_cycle().len() {
        1 => "stable".to_string(),
        period => format!("repeats every {period} generations"),
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
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::SpaceBetween,
                }
                Children [
                    panel_title(Aspect::Rule, "Rule editor"),
                    (
                        #EditorClose
                        @FeathersButton {
                            @caption: bsn! { Text("Close") ThemedText }
                        }
                        on(|_: On<Activate>, mut editor: ResMut<RuleEditor>| editor.toggle())
                    ),
                ]
            ),
            caption("Each case is a 2×2 block before → after one step. A reversible rule is a permutation of the 16 blocks, so you edit it by swapping: click one outcome, then another, to exchange them."),
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
                        @FeathersButton {
                            @caption: bsn! { Text("Identity") ThemedText }
                        }
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
                        @FeathersButton {
                            @caption: bsn! { Text("Inverse") ThemedText }
                        }
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
                        #RuleRandom
                        @FeathersButton {
                            @caption: bsn! { Text("Random") ThemedText }
                        }
                        Node { flex_grow: 1.0 }
                        on(|_: On<Activate>,
                            mut rng: ResMut<Rng>,
                            mut universe: ResMut<Universe>,
                            mut editor: ResMut<RuleEditor>| {
                            universe.set_rule(BlockRule::random(|| rng.next_u64()));
                            editor.say("A random permutation.", universe.rule());
                        })
                    ),
                ]
            ),
            (readout("") template_value(EditorText::Properties)),
            section("RULE STRING", bsn_list![
                (
                    @FeathersTextInputContainer
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
                        flex_direction: FlexDirection::Row,
                        column_gap: px(6),
                    }
                    Children [
                        (
                            #RuleCopy
                            @FeathersButton {
                                @caption: bsn! { Text("Copy") ThemedText }
                            }
                            Node { flex_grow: 1.0 }
                            on(copy_rule)
                        ),
                        (
                            #RulePaste
                            @FeathersButton {
                                @caption: bsn! { Text("Paste") ThemedText }
                            }
                            Node { flex_grow: 1.0 }
                            on(paste_rule)
                        ),
                    ]
                ),
                caption("The outcome of each block, block 0 first (cells count 1, 2, 4, 8: top-left, top-right, bottom-left, bottom-right). Type or paste a table, a preset name, or Morita's number of a rule, like espca-01c5ef."),
                (caption("") template_value(EditorText::Status)),
            ]),
        ])
        EditorPanel
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
            (
                Text("→")
                TextFont {
                    font: FontSourceTemplate::Handle(fonts::REGULAR),
                    font_size: FontSize::Px(12.0),
                    weight: FontWeight::NORMAL,
                }
                TextColor(palette::LIGHT_GRAY_2)
            ),
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
    let cell = BlockCell {
        input,
        bit,
        outcome,
    };
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
            editor.say(
                format!("Swapped the outcomes of blocks {first} and {input}."),
                universe.rule(),
            );
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
fn use_monospace(
    add: On<Add, RuleStringInput>,
    assets: Res<AssetServer>,
    mut commands: Commands,
) {
    commands.entity(add.entity).insert(TextFont {
        font: FontSource::Handle(assets.load(fonts::MONO)),
        font_size: FontSize::Px(12.0),
        ..default()
    });
}

/// A new rule: keep it for "Custom…" if it is hand-made, and drop what was about the old one.
fn follow_rule(universe: Res<Universe>, mut editor: ResMut<RuleEditor>) {
    let rule = universe.rule();
    if rule.preset().is_none() {
        editor.custom = Some(rule.clone());
    }
    editor.selected = None;
    if editor.note.as_ref().is_some_and(|(_, about)| about != rule) {
        editor.note = None;
    }
}

/// Redraws the panel when the rule, the editor state or the hovered outcome changed.
fn sync_editor(
    editor: Res<RuleEditor>,
    universe: Res<Universe>,
    mut panel: Single<&mut Node, With<EditorPanel>>,
    mut cells: Query<
        (&BlockCell, &mut BackgroundColor),
        (Without<CaseCard>, Without<Outcome>),
    >,
    mut cards: Query<(&CaseCard, &mut BackgroundColor), (Without<BlockCell>, Without<Outcome>)>,
    mut outcomes: Query<
        (&Outcome, &Hovered, &mut BackgroundColor),
        (Without<BlockCell>, Without<CaseCard>),
    >,
    mut texts: Query<(&EditorText, &mut Text)>,
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
        color.0 = if table[card.0 as usize] != card.0 {
            palette::GRAY_2
        } else {
            Color::NONE
        };
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
    for (kind, mut text) in &mut texts {
        let content = match kind {
            EditorText::Properties => properties(universe.rule()),
            EditorText::Status => editor
                .typing_error
                .clone()
                .or_else(|| editor.note.as_ref().map(|(message, _)| message.clone()))
                .unwrap_or_default(),
        };
        text.set_if_neq(Text(content));
    }
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
                assert_eq!(crate::rules::rotate_cw(pair[0]), pair[1]);
            }
        }
    }

    #[test]
    fn rules_are_described() {
        let single_rotation: BlockRule = "single-rotation".parse().unwrap();
        assert!(describe(&single_rotation).starts_with("Blocks with exactly one live cell"));
        assert_eq!(
            describe(&BlockRule::identity()),
            "A custom rule, ESPCA-08cadf. Population: conserved. Symmetry: all rotations and \
             mirrors. Vacuum: stable. Reversed: the same rule."
        );
        let mut lopsided = BlockRule::identity();
        lopsided.swap_outcomes(1, 3);
        assert_eq!(
            describe(&lopsided),
            "A custom rule. Population: not conserved. Symmetry: none. Vacuum: stable. \
             Reversed: the same rule."
        );
        let critters: BlockRule = "critters".parse().unwrap();
        assert_eq!(
            properties(&critters),
            "population  conserved relative to the vacuum\n\
             symmetry    all rotations and mirrors\n\
             two states  not interchangeable\n\
             vacuum      repeats every 2 generations\n\
             reversed    the rule complemented\n\
             espca       f7ca80"
        );
        assert!(!properties(&lopsided).contains("espca"));
    }
}
