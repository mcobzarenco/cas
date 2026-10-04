//! The rule library: the rules that go by a name, in a panel to find one, to go through them
//! on the grid one after another, and to keep the rule that is on the grid.
//!
//! The rules and their file are the core's ([`cas_core::library`]). Here are the panel, the
//! few rules the rule menu offers (the pinned ones, and those that were on the grid of late),
//! and the keeping of the file ([`Synced`]): it is written at every change, and read again
//! whenever something else wrote it, a search that keeps its finds or an editor.

use std::path::PathBuf;

use bevy::{
    feathers::{
        constants::fonts,
        controls::{FeathersButton, FeathersTextInput},
        palette,
        theme::ThemedText,
    },
    input_focus::{FocusCause, InputFocus},
    picking::hover::Hovered,
    prelude::*,
    text::{EditableText, FontSourceTemplate, FontWeight, LineBreak, TextEdit, TextEditChange},
    ui::UiGlobalTransform,
    ui_widgets::Activate,
};

use cas_core::{
    families::Constraint,
    library::{Entry, Library, properties},
    rules::{BlockRule, Source},
    universe::Universe,
};

use crate::{
    kit::{
        Aspect, Scrolls, caption, chip_box, field_frame, heading, icons, list_row, panel_title, scrolling, side_panel,
    },
    sampler::{CHIPS, chip_face},
    sim::SimSystems,
    synced::Synced,
};

pub const LIBRARY_WIDTH: f32 = 376.0;
/// So many of the rules that were on the grid of late are remembered, and so many of them
/// the rule menu offers.
const REMEMBERED: usize = 16;
const OFFERED: usize = 6;
/// The outcomes of so many blocks, of the sixteen, name a rule that has no name.
const TABLE_SHOWN: usize = 10;
/// Every so many seconds the file is looked at, for what something else wrote there.
pub const LOOKS_EVERY: f32 = 0.5;

#[derive(Resource)]
pub struct RuleLibrary {
    open: bool,
    /// The library, and the file the kept rules are in. A scripted run has none, and keeps
    /// to itself.
    library: Synced<Library>,
    /// The rules that were on the grid of late, the latest first. One that comes back keeps
    /// its place: going through them does not shuffle them.
    recent: Vec<BlockRule>,
    /// What the list is narrowed to: the words typed, and the properties asked for with the
    /// chips, which are folded away unless `choosing`.
    filter: String,
    wanted: [bool; CHIPS.len()],
    choosing: bool,
    /// The name typed for a rule that is not kept yet.
    draft: String,
    /// What the last button did, or what is wrong.
    note: Option<String>,
    /// Counts the changes that show, in the list or in the rule menu.
    revision: u64,
    /// The rules of the list as it is shown, from top to bottom: what the arrow keys go
    /// through, and what a click on a row picks.
    rows: Vec<BlockRule>,
    /// The field the keyboard goes to next: the name of a rule just kept.
    focus: Option<Field>,
    /// For so many frames yet, the list is scrolled to where the rule on the grid is.
    reveal: u8,
    /// The list goes back to its top: it was narrowed to other rules.
    top: bool,
}

/// How the rule on the grid stands with the library.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Standing {
    /// It came with the program, as this entry.
    BuiltIn(usize),
    /// It was kept, as this entry.
    Kept(usize),
    /// It is not in the library.
    Loose,
}

impl RuleLibrary {
    /// The library of the file at `path`, or the built-in rules alone where there is no such
    /// file yet. Without a path the library is written nowhere.
    pub fn at(path: Option<PathBuf>) -> Self {
        Self {
            open: false,
            library: Synced::at(path),
            recent: Vec::new(),
            filter: String::new(),
            wanted: [false; CHIPS.len()],
            choosing: false,
            draft: String::new(),
            note: None,
            revision: 0,
            rows: Vec::new(),
            focus: None,
            reveal: 0,
            top: false,
        }
    }

    /// Reads the file if something else wrote it: true if it was read.
    fn read_again(&mut self) -> bool {
        let read = self.library.read_again();
        self.revision += read as u64;
        read
    }

    /// Changes the library, in the file as well, if `change` says that it did change
    /// something.
    fn edit(&mut self, change: impl FnOnce(&mut Library) -> bool) {
        // The file may have been read again before the change, which shows too.
        self.library.edit(change);
        self.revision += 1;
    }

    pub fn toggle(&mut self) {
        self.open = !self.open;
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn show(&mut self) {
        if !self.open {
            self.open = true;
        }
    }

    pub fn close(&mut self) {
        if self.open {
            self.open = false;
        }
    }

    /// What tells a change of the list or of the rule menu from none.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The entry of a rule, if it has one.
    pub fn entry(&self, rule: &BlockRule) -> Option<&Entry> {
        self.library.of(rule).map(|index| &self.library.entries()[index])
    }

    /// What a rule goes by: its name in the library, or Morita's number, or the beginning of
    /// its table.
    pub fn label(&self, rule: &BlockRule) -> String {
        match (self.entry(rule), rule.espca()) {
            (Some(entry), _) => entry.name.clone(),
            (None, Some(number)) => format!("ESPCA-{number}"),
            (None, None) => {
                let outcomes: Vec<String> = rule.table()[..TABLE_SHOWN].iter().map(u8::to_string).collect();
                format!("{}…", outcomes.join(","))
            }
        }
    }

    /// The rules the rule menu offers: the pinned ones, as the library has them, and the
    /// latest of the others that were on the grid.
    pub fn offered(&self) -> (Vec<&Entry>, Vec<&BlockRule>) {
        let pinned: Vec<&Entry> = self.library.entries().iter().filter(|entry| entry.pinned).collect();
        let is_pinned = |rule: &&BlockRule| pinned.iter().any(|entry| entry.rule == **rule);
        let recent = self.recent.iter().filter(|rule| !is_pinned(rule)).take(OFFERED).collect();
        (pinned, recent)
    }

    fn standing(&self, rule: &BlockRule) -> Standing {
        match self.library.of(rule) {
            Some(index) if self.library.entries()[index].kept() => Standing::Kept(index),
            Some(index) => Standing::BuiltIn(index),
            None => Standing::Loose,
        }
    }

    /// A rule is on the grid: it is remembered among the latest. When those are too many, a
    /// rule of the library is let go of before one that has no name: that one is found
    /// nowhere else.
    fn saw(&mut self, rule: &BlockRule) {
        if !self.recent.contains(rule) {
            self.recent.insert(0, rule.clone());
            while self.recent.len() > REMEMBERED {
                let named = self.recent.iter().rposition(|rule| self.library.of(rule).is_some());
                self.recent.remove(named.unwrap_or(REMEMBERED));
            }
        }
        // What was said was about the rule before.
        self.draft.clear();
        self.note = None;
        self.revision += 1;
    }

    /// Keeps the rule on the grid, under the name typed for it or one to be going on with,
    /// and has the name taken next: typing gives it a better one.
    pub fn keep(&mut self, rule: &BlockRule) {
        let typed = self.draft.trim().to_string();
        let mut said = String::new();
        let mut kept = false;
        self.edit(|library| {
            let name = if typed.is_empty() { library.unused("Unnamed") } else { typed };
            said = match library.keep(rule.clone(), &name) {
                Ok(_) => {
                    kept = true;
                    format!("Kept as “{name}”.")
                }
                Err(known) => {
                    let entry = &library.entries()[known];
                    let how =
                        if entry.rule == *rule { "" } else { ", in another form: turned, mirrored or begun later" };
                    format!("It is in the library already, as “{}”{how}.", entry.name)
                }
            };
            kept
        });
        if kept {
            self.focus = Some(Field::Name);
        }
        self.note = Some(said);
        self.open = true;
        self.revision += 1;
    }

    /// Pins a rule of the library to the rule menu, or lets it go from there.
    fn pin(&mut self, rule: &BlockRule) {
        self.edit(|library| match library.of(rule) {
            Some(entry) => {
                library.pin(entry, !library.entries()[entry].pinned);
                true
            }
            None => false,
        });
    }

    /// Forgets a rule that was kept.
    fn forget(&mut self, rule: &BlockRule) {
        let mut forgotten = None;
        self.edit(|library| {
            let Some(entry) = library.of(rule) else {
                return false;
            };
            forgotten = Some(library.entries()[entry].name.clone());
            library.forget(entry)
        });
        if let Some(name) = forgotten.filter(|_| self.library.of(rule).is_none()) {
            self.note = Some(format!("Forgot “{name}”."));
        }
    }

    /// Writes what was typed in a field about a rule that was kept: its name, its tags or
    /// its note.
    fn write_about(&mut self, rule: &BlockRule, field: Field, typed: &str) {
        self.edit(|library| {
            let Some(entry) = library.of(rule).filter(|&entry| library.entries()[entry].kept()) else {
                return false;
            };
            match field {
                Field::Name => library.rename(entry, typed),
                Field::Tags => library.tag(entry, typed),
                Field::Note => library.annotate(entry, typed),
                Field::Filter => return false,
            }
            true
        });
        // What a button said was said before this.
        self.note = None;
    }

    /// The rule that comes so many rows after the one on the grid in the list as it is shown:
    /// the first or the last of the list for a rule that is not in it.
    pub fn step(&mut self, current: &BlockRule, by: i32) -> Option<BlockRule> {
        let last = self.rows.len().checked_sub(1)?;
        let next = match self.rows.iter().position(|rule| rule == current) {
            Some(at) => at.saturating_add_signed(by as isize).min(last),
            None if by > 0 => 0,
            None => last,
        };
        self.reveal = 3;
        Some(self.rows[next].clone())
    }

    /// The properties asked for with the chips.
    fn properties(&self) -> Vec<Constraint> {
        CHIPS.iter().zip(self.wanted).filter(|(_, wanted)| *wanted).map(|(chip, _)| chip.constraint).collect()
    }
}

/// The panel.
#[derive(Component, Default, Clone)]
struct LibraryPanel;

/// The node that holds the rows of the list, and scrolls.
#[derive(Component, Default, Clone)]
struct RuleList;

/// A row of the list: which of the rules shown it is.
#[derive(Component, Default, Clone, Copy)]
struct RuleRow(usize);

/// The pin of a row: which entry of the library it pins.
#[derive(Component, Default, Clone, Copy)]
struct RowPin(usize);

/// The pin of the rule on the grid, in the card about it.
#[derive(Component, Default, Clone)]
struct CardPin;

/// A chip that narrows the list to the rules with a property, and its sign; the button that
/// unfolds the chips, with its mark; and the box they are in.
#[derive(Component, Default, Clone, Copy)]
struct Has(usize);

#[derive(Component, Default, Clone, Copy)]
struct HasSign(usize);

#[derive(Component, Default, Clone)]
struct Unfolds;

#[derive(Component, Default, Clone)]
struct UnfoldMark;

#[derive(Component, Default, Clone)]
struct ChipBox;

/// When a part of the card about the rule on the grid is there: for a rule that came with
/// the program, for one that was kept, for one that is in the library either way, for one
/// that is not, and for one that can be given a name.
#[derive(Component, Default, Clone, Copy, PartialEq, Eq)]
enum When {
    #[default]
    BuiltIn,
    Kept,
    Listed,
    Loose,
    Named,
}

impl When {
    fn is_now(self, standing: Standing) -> bool {
        match (self, standing) {
            (When::BuiltIn, Standing::BuiltIn(_))
            | (When::Kept, Standing::Kept(_))
            | (When::Loose, Standing::Loose) => true,
            (When::Listed, standing) => standing != Standing::Loose,
            (When::Named, standing) => !matches!(standing, Standing::BuiltIn(_)),
            _ => false,
        }
    }
}

/// The texts of the panel that follow the rule on the grid, and the line that says what the
/// last button did.
#[derive(Component, Default, Clone, Copy, PartialEq, Eq)]
enum Says {
    #[default]
    Name,
    About,
    Status,
}

/// The text fields of the panel: what narrows the list, and the name, the tags and the note
/// of the rule on the grid.
#[derive(Component, Default, Clone, Copy, PartialEq, Eq, Debug)]
enum Field {
    #[default]
    Filter,
    Name,
    Tags,
    Note,
}

pub struct LibraryPlugin;

impl Plugin for LibraryPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, follow_rule.in_set(SimSystems::Input)).add_systems(
            Update,
            (watch_file, sync_fields, show_library, light_chips, list_rules, reveal)
                .chain()
                .in_set(SimSystems::Present),
        );
    }
}

pub fn library_panel() -> impl Scene {
    let chips: Vec<_> = (0..CHIPS.len()).map(property_chip).collect();
    bsn! {
        #RuleLibrary
        side_panel(LIBRARY_WIDTH, bsn_list![
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::SpaceBetween,
                }
                Children [
                    panel_title(Aspect::Rule, "Rule library"),
                    (
                        #LibraryClose
                        @FeathersButton {
                            @caption: bsn! { Text("Close") ThemedText }
                        }
                        on(|_: On<Activate>, mut library: ResMut<RuleLibrary>| library.toggle())
                    ),
                ]
            ),
            on_the_grid(),
            (
                // What the list is narrowed to.
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(8),
                    flex_shrink: 0.0,
                }
                Children [
                    (heading("FIND") Node { flex_shrink: 0.0 }),
                    (
                        field_frame()
                        Node { flex_basis: px(0), min_width: px(0) }
                        Children [ field("LibraryFind", Field::Filter) ]
                    ),
                    (
                        // Unfolds the properties to ask for; outlined while any is asked for.
                        #LibraryHas
                        chip_box()
                        Node { flex_shrink: 0.0 }
                        Unfolds
                        on(|_: On<Pointer<Click>>, mut library: ResMut<RuleLibrary>| library.choosing = !library.choosing)
                        Children [
                            (
                                icons::icon(icons::OPENS, 11.0, palette::LIGHT_GRAY_2)
                                UiTransform
                                UnfoldMark
                                template_value(Pickable::IGNORE)
                            ),
                            (caption("has") template_value(Pickable::IGNORE)),
                        ]
                    ),
                ]
            ),
            (
                #LibraryChips
                Node {
                    display: Display::None,
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    align_items: AlignItems::Center,
                    column_gap: px(5),
                    row_gap: px(5),
                    flex_shrink: 0.0,
                }
                ChipBox
                Children [ { chips } ]
            ),
            scrolling(Scrolls::Rows, bsn! { #LibraryList RuleList }),
            (#LibraryStatus caption("") template_value(Says::Status)),
        ])
        LibraryPanel
    }
}

/// The card about the rule on the grid: its name and its pin if it is in the library, with
/// its tags and its note to write if it was kept; and a name to keep it under if it is not.
fn on_the_grid() -> impl Scene {
    bsn! {
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(6),
            padding: px(8),
            border_radius: px(5),
            flex_shrink: 0.0,
        }
        BackgroundColor(palette::GRAY_2)
        Children [
            heading("ON THE GRID"),
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(6),
                }
                Children [
                    (
                        #LibraryPin
                        @FeathersButton {
                            @caption: bsn! { icons::icon(icons::PIN, 14.0, palette::LIGHT_GRAY_2) CardPin }
                        }
                        Node {
                            display: Display::None,
                            width: px(26),
                            min_width: px(26),
                            padding: px(0),
                            justify_content: JustifyContent::Center,
                            flex_shrink: 0.0,
                        }
                        template_value(When::Listed)
                        on(pin_this)
                    ),
                    (
                        #LibraryName
                        Text("")
                        TextFont {
                            font: FontSourceTemplate::Handle(fonts::REGULAR),
                            font_size: FontSize::Px(14.0),
                            weight: FontWeight::NORMAL,
                        }
                        TextColor(palette::WHITE)
                        Node { display: Display::None, flex_grow: 1.0, flex_basis: px(0) }
                        template_value(Says::Name)
                        template_value(When::BuiltIn)
                    ),
                    (
                        field_frame()
                        Node { display: Display::None, flex_basis: px(0), min_width: px(0) }
                        template_value(When::Named)
                        Children [ field("KeptName", Field::Name) ]
                    ),
                    (
                        #LibraryKeep
                        @FeathersButton {
                            @caption: bsn! { Text("Keep") ThemedText }
                        }
                        Node { display: Display::None, flex_shrink: 0.0 }
                        template_value(When::Loose)
                        on(|_: On<Activate>, universe: Res<Universe>, mut library: ResMut<RuleLibrary>| {
                            library.keep(universe.rule());
                        })
                    ),
                    (
                        #LibraryForget
                        @FeathersButton {
                            @caption: bsn! { Text("Forget") ThemedText }
                        }
                        Node { display: Display::None, flex_shrink: 0.0 }
                        template_value(When::Kept)
                        on(forget_this)
                    ),
                ]
            ),
            (labelled("TAGS", "KeptTags", Field::Tags) template_value(When::Kept)),
            (labelled("NOTE", "KeptNote", Field::Note) template_value(When::Kept)),
            (#LibraryAbout caption("") template_value(Says::About)),
        ]
    }
}

/// A chip that narrows the list to the rules that have a property: the chips a random rule is
/// asked for with, in the editor.
fn property_chip(index: usize) -> impl Scene {
    let name = Name::new(format!("Has:{}", CHIPS[index].name));
    let has = Has(index);
    bsn! {
        chip_face(index, HasSign(index))
        template_value(name)
        template_value(has)
        on(|click: On<Pointer<Click>>, chips: Query<&Has>, mut library: ResMut<RuleLibrary>| {
            if let Ok(&Has(index)) = chips.get(click.entity) {
                library.wanted[index] = !library.wanted[index];
                library.top = true;
                library.revision += 1;
            }
        })
    }
}

/// Lights the chips that are on, and the button that unfolds them while any is; and shows
/// the chips or folds them away.
fn light_chips(
    library: Res<RuleLibrary>,
    mut chips: Query<(&Has, &Hovered, &mut BackgroundColor, &mut BorderColor), Without<Unfolds>>,
    mut signs: Query<(&HasSign, &mut TextColor)>,
    mut unfolds: Single<(&Hovered, &mut BackgroundColor, &mut BorderColor), With<Unfolds>>,
    mut mark: Single<&mut UiTransform, With<UnfoldMark>>,
    mut chip_box: Single<&mut Node, With<ChipBox>>,
) {
    if !library.open {
        return;
    }
    let fill = |hovered: &Hovered| BackgroundColor(if hovered.0 { palette::GRAY_3 } else { palette::GRAY_2 });
    let outline = |on: bool| BorderColor::all(if on { Aspect::Rule.color() } else { Color::NONE });
    for (&Has(index), hovered, mut background, mut border) in &mut chips {
        background.set_if_neq(fill(hovered));
        border.set_if_neq(outline(library.wanted[index]));
    }
    for (&HasSign(index), mut color) in &mut signs {
        let ink = if library.wanted[index] { palette::WHITE } else { palette::LIGHT_GRAY_2 };
        color.set_if_neq(TextColor(ink));
    }
    let (hovered, background, border) = &mut *unfolds;
    background.set_if_neq(fill(hovered));
    border.set_if_neq(outline(library.wanted.contains(&true)));
    let turned = if library.choosing { Rot2::FRAC_PI_2 } else { Rot2::IDENTITY };
    if mark.rotation != turned {
        mark.rotation = turned;
    }
    let display = if library.choosing { Display::Flex } else { Display::None };
    if chip_box.display != display {
        chip_box.display = display;
    }
}

/// A text field of the panel.
fn field(name: &'static str, field: Field) -> impl Scene {
    let name = Name::new(name);
    bsn! {
        @FeathersTextInput {}
        template_value(name)
        template_value(field)
        on(field_edited)
    }
}

/// A text field with its name in front of it.
fn labelled(label: &'static str, name: &'static str, which: Field) -> impl Scene {
    bsn! {
        Node {
            display: Display::None,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(8),
        }
        Children [
            (heading(label) Node { width: px(34), flex_shrink: 0.0 }),
            (
                field_frame()
                Node { flex_basis: px(0), min_width: px(0) }
                Children [ field(name, which) ]
            ),
        ]
    }
}

/// A row of the list: a rule by its name, with a line more about it, and its pin if it is in
/// the library. The row of the rule on the grid is outlined.
fn rule_row(position: usize, title: String, about: String, pin: Option<(usize, bool)>, current: bool) -> impl Scene {
    let name = Name::new(format!("LibraryRow{position}"));
    let pin_name = Name::new(format!("LibraryRowPin{position}"));
    let row = RuleRow(position);
    let outline = if current { Aspect::Rule.color() } else { Color::NONE };
    let (pinned, shown) = match pin {
        Some((entry, pinned)) => ((RowPin(entry), pinned), Display::Flex),
        None => ((RowPin(usize::MAX), false), Display::None),
    };
    let ink = if pinned.1 { Aspect::Rule.color() } else { palette::GRAY_3 };
    let pin = pinned.0;
    let second = if about.is_empty() { Display::None } else { Display::Flex };
    bsn! {
        list_row()
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(6),
            padding: UiRect::axes(px(8), px(5)),
        }
        BorderColor::all(outline)
        template_value(name)
        template_value(row)
        on(pick_row)
        Children [
            (
                Node {
                    flex_grow: 1.0,
                    flex_basis: px(0),
                    min_width: px(0),
                    overflow: Overflow::clip(),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(2),
                }
                template_value(Pickable::IGNORE)
                Children [
                    (
                        Text(title)
                        TextFont {
                            font: FontSourceTemplate::Handle(fonts::REGULAR),
                            font_size: FontSize::Px(13.0),
                            weight: FontWeight::NORMAL,
                        }
                        TextColor(palette::WHITE)
                        TextLayout { linebreak: LineBreak::NoWrap }
                        template_value(Pickable::IGNORE)
                    ),
                    (
                        caption(about)
                        TextLayout { linebreak: LineBreak::NoWrap }
                        Node { display: second }
                        template_value(Pickable::IGNORE)
                    ),
                ]
            ),
            (
                // Pins the rule to the rule menu, or lets it go from there.
                Node {
                    display: shown,
                    width: px(24),
                    height: px(24),
                    flex_shrink: 0.0,
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border_radius: px(4),
                }
                Hovered
                template_value(pin_name)
                template_value(pin)
                on(pin_row)
                Children [ (icons::icon(icons::PIN, 14.0, ink) template_value(Pickable::IGNORE)) ]
            ),
        ]
    }
}

/// A click on a row puts its rule on the grid.
fn pick_row(
    click: On<Pointer<Click>>,
    rows: Query<&RuleRow>,
    library: Res<RuleLibrary>,
    mut universe: ResMut<Universe>,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    if let Some(rule) = rows.get(click.entity).ok().and_then(|&RuleRow(position)| library.rows.get(position)) {
        universe.set_rule(rule.clone());
    }
}

/// The pin of a row pins its rule to the rule menu, or lets it go. The click goes no
/// further: the row would put the rule on the grid.
fn pin_row(mut click: On<Pointer<Click>>, pins: Query<&RowPin>, mut library: ResMut<RuleLibrary>) {
    let Ok(&RowPin(entry)) = pins.get(click.entity) else {
        return;
    };
    click.propagate(false);
    if click.button == PointerButton::Primary
        && let Some(rule) = library.library.entries().get(entry).map(|entry| entry.rule.clone())
    {
        library.pin(&rule);
    }
}

/// The pin of the card does the same for the rule on the grid.
fn pin_this(_: On<Activate>, universe: Res<Universe>, mut library: ResMut<RuleLibrary>) {
    library.pin(universe.rule());
}

/// Forgets the rule on the grid, if it was kept: it stays on the grid, and among the latest.
fn forget_this(_: On<Activate>, universe: Res<Universe>, mut library: ResMut<RuleLibrary>) {
    library.forget(universe.rule());
}

/// A rule that comes on the grid is remembered among the latest.
fn follow_rule(universe: Res<Universe>, mut library: ResMut<RuleLibrary>, mut seen: Local<Option<BlockRule>>) {
    if seen.as_ref() != Some(universe.rule()) {
        *seen = Some(universe.rule().clone());
        library.saw(universe.rule());
    }
}

/// Typing in a field of the panel: the list is narrowed as the words come, and what is
/// written of a kept rule is written as it is typed.
fn field_edited(
    _: On<TextEditChange>,
    fields: Query<(&EditableText, &Field)>,
    focus: Res<InputFocus>,
    universe: Res<Universe>,
    mut library: ResMut<RuleLibrary>,
) {
    // The fields are also rewritten when the rule changes; only the user's edits count.
    let Some((text, &field)) = focus.get().and_then(|focused| fields.get(focused).ok()) else {
        return;
    };
    let typed = text.value().to_string();
    let standing = library.standing(universe.rule());
    // Moving the caret reports an edit as well, so most of the time nothing is new.
    if typed == library.bypass_change_detection().written(field, standing) {
        return;
    }
    match (field, standing) {
        (Field::Filter, _) => {
            library.filter = typed;
            library.top = true;
            library.revision += 1;
        }
        (_, Standing::Kept(_)) => library.write_about(universe.rule(), field, &typed),
        (Field::Name, _) => library.draft = typed,
        _ => {}
    }
}

impl RuleLibrary {
    /// What a field holds, by what is known: the words the list is narrowed to, and the name,
    /// the tags and the note of the rule on the grid.
    fn written(&self, field: Field, standing: Standing) -> String {
        let entry = match standing {
            Standing::Kept(entry) => Some(&self.library.entries()[entry]),
            _ => None,
        };
        match (field, entry) {
            (Field::Filter, _) => self.filter.clone(),
            (Field::Name, Some(entry)) => entry.name.clone(),
            (Field::Name, None) => self.draft.clone(),
            (Field::Tags, Some(entry)) => entry.tags.join(", "),
            (Field::Note, Some(entry)) => entry.note.clone(),
            _ => String::new(),
        }
    }
}

/// Keeps the fields saying what is known, unless the user is typing in them; and hands the
/// keyboard to the name of a rule that was just kept, all of it selected, to be typed over.
fn sync_fields(
    mut library: ResMut<RuleLibrary>,
    universe: Res<Universe>,
    mut focus: ResMut<InputFocus>,
    mut fields: Query<(Entity, &Field, &mut EditableText)>,
    mut shown: Local<Option<(u64, BlockRule)>>,
) {
    let wanted = library.bypass_change_detection().focus.take();
    let now = (library.revision, universe.rule().clone());
    if shown.as_ref() == Some(&now) && !focus.is_changed() && wanted.is_none() {
        return;
    }
    *shown = Some(now);
    let standing = library.standing(universe.rule());
    for (entity, &field, mut text) in &mut fields {
        if focus.get() == Some(entity) {
            continue;
        }
        let written = library.written(field, standing);
        if text.value().to_string() != written {
            text.queue_edit(TextEdit::SelectAll);
            text.queue_edit(TextEdit::Insert(written.as_str().into()));
            text.queue_edit(TextEdit::TextStart(false));
        }
        if wanted == Some(field) {
            focus.set(entity, FocusCause::Navigated);
            text.queue_edit(TextEdit::SelectAll);
        }
    }
}

/// Shows or hides the panel, and keeps the card about the rule on the grid and the line at
/// the bottom in step.
fn show_library(
    library: Res<RuleLibrary>,
    universe: Res<Universe>,
    panel: Single<Entity, With<LibraryPanel>>,
    parts: Query<(Entity, &When)>,
    mut nodes: Query<&mut Node>,
    mut texts: Query<(Entity, &Says, &mut Text)>,
    mut pin: Single<&mut TextColor, With<CardPin>>,
    mut shown: Local<Option<(u64, BlockRule, bool, Option<String>)>>,
) {
    let now = (library.revision, universe.rule().clone(), library.open, library.note.clone());
    if shown.as_ref() == Some(&now) {
        return;
    }
    *shown = Some(now);
    let mut show = |entity: Entity, shown: bool| {
        let display = if shown { Display::Flex } else { Display::None };
        if let Ok(mut node) = nodes.get_mut(entity)
            && node.display != display
        {
            node.display = display;
        }
    };
    show(*panel, library.open);
    let rule = universe.rule();
    let standing = library.standing(rule);
    for (part, when) in &parts {
        show(part, when.is_now(standing));
    }
    let entry = library.entry(rule);
    // On the face of its button, the pin of a rule that is not pinned is grey, not dark.
    let ink = if entry.is_some_and(|entry| entry.pinned) { Aspect::Rule.color() } else { palette::LIGHT_GRAY_2 };
    pin.set_if_neq(TextColor(ink));
    for (line, says, mut text) in &mut texts {
        let said = match (says, standing) {
            (Says::Name, _) => entry.map_or(String::new(), |entry| entry.name.clone()),
            (Says::About, Standing::BuiltIn(_)) => entry.map_or(String::new(), |entry| entry.note.clone()),
            (Says::About, Standing::Kept(_)) => String::new(),
            (Says::About, Standing::Loose) => match library.library.twin(rule) {
                Some(twin) => format!(
                    "Not in the library as it is: it is “{}” in another form, turned, mirrored or begun later.",
                    library.library.entries()[twin].name
                ),
                None => "Not in the library. Keep puts it there, under the name typed.".to_string(),
            },
            // What is wrong with the file is said for as long as it is.
            (Says::Status, _) => library.library.trouble().map(str::to_string).or(library.note.clone()).unwrap_or(
                "A click puts a rule on the grid, and ↑ and ↓ go through the list. Words narrow it: of a name, a \
                 tag, a note, or what a rule has, as conserving or half-turn."
                    .to_string(),
            ),
        };
        // The name is there for a built-in rule, which a kept one has a field for.
        if *says != Says::Name {
            show(line, !said.is_empty());
        }
        text.set_if_neq(Text(said));
    }
}

/// What a row says of a rule that is in the library, under its name: its tags and the
/// beginning of its note, or, where nothing was written, what the rule has.
fn about(entry: &Entry) -> String {
    match (entry.tags.is_empty(), entry.note.is_empty()) {
        (true, true) => properties(&entry.rule).join(" · "),
        (true, false) => entry.note.clone(),
        (false, true) => entry.tags.join(" · "),
        (false, false) => format!("{} — {}", entry.tags.join(" · "), entry.note),
    }
}

/// Lists the rules: those that were on the grid of late and are not in the library, the kept
/// ones with the latest first, and the built-in ones by where they are from; of the library's,
/// those the words of the field leave.
fn list_rules(
    mut library: ResMut<RuleLibrary>,
    universe: Res<Universe>,
    list: Single<Entity, With<RuleList>>,
    mut shown: Local<Option<(u64, BlockRule)>>,
    mut commands: Commands,
) {
    let now = (library.revision, universe.rule().clone());
    if shown.as_ref() == Some(&now) {
        return;
    }
    *shown = Some(now);
    let current = universe.rule();
    let wanted = library.properties();
    let found = library.library.matching(&library.filter, &wanted);
    let entries = library.library.entries();
    let mut rules: Vec<BlockRule> = Vec::new();
    let mut rows: Vec<Entity> = Vec::new();
    // The latest rules that have no name yet: those with the properties asked for, and none
    // while words are looked for, of which they have none.
    let nameless = |rule: &&BlockRule| library.library.of(rule).is_none() && wanted.iter().all(|has| has.holds(rule));
    let loose: Vec<&BlockRule> = match library.filter.trim() {
        "" => library.recent.iter().filter(nameless).collect(),
        _ => Vec::new(),
    };
    if !loose.is_empty() {
        rows.push(commands.spawn_scene(section("OF LATE, NOT KEPT")).id());
    }
    for rule in loose {
        // A rule without a name goes by its table, and by what it has.
        let twin = library.library.twin(rule).map(|twin| format!("“{}” in another form", entries[twin].name));
        let about = twin.unwrap_or_else(|| properties(rule).join(" · "));
        let row = rule_row(rules.len(), library.label(rule), about, None, rule == current);
        rows.push(commands.spawn_scene(row).id());
        rules.push(rule.clone());
    }
    let kept = found.iter().rev().filter(|&&index| entries[index].kept());
    let from = |source: Source| found.iter().filter(move |&&index| entries[index].source == Some(source));
    let sections: [(&str, Vec<usize>); 4] = [
        ("KEPT", kept.copied().collect()),
        ("FROM THE COLLECTIONS", from(Source::Collections).copied().collect()),
        ("FROM MORITA'S BOOK", from(Source::Morita).copied().collect()),
        ("FOUND BY SEARCH", from(Source::Search).copied().collect()),
    ];
    for (title, listed) in sections {
        if !listed.is_empty() {
            rows.push(commands.spawn_scene(section(title)).id());
        }
        for index in listed {
            let entry = &entries[index];
            let pin = Some((index, entry.pinned));
            let row = rule_row(rules.len(), entry.name.clone(), about(entry), pin, entry.rule == *current);
            rows.push(commands.spawn_scene(row).id());
            rules.push(entry.rule.clone());
        }
    }
    if rules.is_empty() {
        rows.push(commands.spawn_scene(caption("No rule of the library has all of that.")).id());
    }
    commands.entity(*list).despawn_related::<Children>();
    commands.entity(*list).add_children(&rows);
    library.bypass_change_detection().rows = rules;
}

/// The name of a group of rows.
fn section(title: &'static str) -> impl Scene {
    bsn! {
        Node { padding: UiRect { left: px(8), top: px(4) }, flex_shrink: 0.0 }
        Children [ heading(title) ]
    }
}

/// After a step through the list with the keys, scrolls the list to where the rule on the
/// grid is, once its row has its place.
fn reveal(
    mut library: ResMut<RuleLibrary>,
    universe: Res<Universe>,
    rows: Query<(&RuleRow, &ComputedNode, &UiGlobalTransform)>,
    mut list: Single<(&ComputedNode, &UiGlobalTransform, &mut ScrollPosition), With<RuleList>>,
) {
    if std::mem::take(&mut library.bypass_change_detection().top) {
        list.2.0.y = 0.0;
    }
    if library.reveal == 0 {
        return;
    }
    library.bypass_change_detection().reveal -= 1;
    let on_the_grid = |&(&RuleRow(position), ..): &(&RuleRow, &ComputedNode, &UiGlobalTransform)| {
        library.rows.get(position) == Some(universe.rule())
    };
    let Some((_, row, at)) = rows.iter().find(on_the_grid).filter(|(_, row, _)| row.size.y > 0.0) else {
        return;
    };
    let (frame, middle, scroll) = &mut *list;
    // Sizes and places are in physical pixels, and the list scrolls in logical ones. A row
    // more is kept in sight on either side, so that the next step shows where it goes.
    let (top, bottom) = (middle.translation.y - 0.5 * frame.size.y, middle.translation.y + 0.5 * frame.size.y);
    let (row_top, row_bottom) = (at.translation.y - 1.5 * row.size.y, at.translation.y + 1.5 * row.size.y);
    let by = if row_top < top { row_top - top } else { (row_bottom - bottom).max(0.0) };
    if by != 0.0 {
        scroll.0.y = (scroll.0.y + by * frame.inverse_scale_factor).max(0.0);
    }
}

/// Looks at the file every now and then, and reads it again if something else wrote it.
fn watch_file(mut library: ResMut<RuleLibrary>, time: Res<Time>, mut since: Local<f32>) {
    *since += time.delta_secs();
    if *since < LOOKS_EVERY {
        return;
    }
    *since = 0.0;
    let library = library.bypass_change_detection();
    if library.read_again() && library.library.readable() {
        library.note = Some("The file of the library was written from outside, and read again.".to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menu_offers_the_pinned_rules_and_the_latest() {
        let mut library = RuleLibrary::at(None);
        let (pinned, recent) = library.offered();
        assert_eq!((pinned.len(), recent.len()), (cas_core::library::PINNED.len(), 0));
        // A pinned rule that comes on the grid is not offered twice; a rule without a name
        // goes by the beginning of its table, or by Morita's number if it has one.
        let critters: BlockRule = "critters".parse().unwrap();
        let loose: BlockRule = "0,2,8,6,1,5,3,7,4,9,10,11,12,13,14,15".parse().unwrap();
        let numbered: BlockRule = "espca-04c5bf".parse().unwrap();
        for rule in [&critters, &loose, &numbered, &loose] {
            library.saw(rule);
        }
        let (_, recent) = library.offered();
        assert_eq!(recent, [&numbered, &loose]);
        // Going through the whole library lets go of none of the rules without a name.
        let all: Vec<BlockRule> = library.library.entries().iter().map(|entry| entry.rule.clone()).collect();
        for rule in &all {
            library.saw(rule);
        }
        assert_eq!(library.recent.len(), REMEMBERED);
        assert!(library.recent.contains(&loose) && library.recent.contains(&numbered));
        assert_eq!(library.label(&critters), "Critters");
        assert_eq!(library.label(&loose), "0,2,8,6,1,5,3,7,4,9…");
        assert_eq!(library.label(&numbered), "ESPCA-04c5bf");
        // Kept, it has a name, and the keys go through the list as it is shown.
        library.draft = "A gun".to_string();
        library.keep(&loose);
        assert_eq!(library.label(&loose), "A gun");
        assert_eq!(library.note.as_deref(), Some("Kept as “A gun”."));
        library.keep(&loose.canonical());
        assert!(library.note.as_deref().is_some_and(|note| note.contains("as “A gun”, in another form")));
        library.rows = vec![critters.clone(), loose.clone()];
        assert_eq!(library.step(&critters, 1), Some(loose.clone()));
        assert_eq!(library.step(&loose, 1), Some(loose.clone()));
        assert_eq!(library.step(&numbered, -1), Some(loose.clone()));
        assert_eq!(library.step(&numbered, 1), Some(critters));
    }

    #[test]
    fn the_file_is_written_at_every_change_and_read_again_when_something_else_wrote_it() {
        let folder = std::env::temp_dir().join(format!("cas-rule-library-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("rules.tsv");
        let on_disk = || Library::read(&path).unwrap();
        let rule = |table: &str| table.parse::<BlockRule>().unwrap();
        let gun = rule("0,2,8,6,1,5,3,7,4,9,10,11,12,13,14,15");
        let (first, second) =
            (rule("0,4,8,3,1,5,6,7,2,9,10,11,12,13,14,15"), rule("0,8,1,3,2,5,6,7,4,9,10,11,12,13,14,15"));

        // There is no file until something is kept or pinned. A rule kept without a name gets
        // one that is free.
        let mut library = RuleLibrary::at(Some(path.clone()));
        assert!(!path.exists());
        library.keep(&gun);
        assert_eq!(library.note.as_deref(), Some("Kept as “Unnamed 1”."));
        assert_eq!(on_disk(), *library.library);
        library.write_about(&gun, Field::Name, "A gun");
        library.write_about(&gun, Field::Tags, "gun, found");
        let kept = on_disk();
        let entry = &kept.entries()[kept.of(&gun).unwrap()];
        assert_eq!((entry.name.as_str(), entry.tags.len()), ("A gun", 2));

        // A search keeps a find in the file. The library here is the file's again when it is
        // looked at.
        let mut outside = on_disk();
        outside.keep(first.clone(), "A find").unwrap();
        outside.write(&path).unwrap();
        assert!(library.read_again() && !library.read_again());
        assert_eq!(library.label(&first), "A find");
        // And a change made here before the file was looked at is made to the file as it is:
        // nothing of what was written there is lost.
        outside.keep(second.clone(), "Another find").unwrap();
        outside.write(&path).unwrap();
        library.pin(&gun);
        library.forget(&first);
        assert_eq!(library.note.as_deref(), Some("Forgot “A find”."));
        let file = on_disk();
        assert_eq!(file, *library.library);
        assert!(file.entries()[file.of(&gun).unwrap()].pinned);
        assert_eq!((file.of(&first), file.of(&second).is_some()), (None, true));
        // Only what was kept is forgotten.
        library.forget(&rule("critters"));
        assert_eq!(on_disk(), file);

        // A file that is no library is left as it is, and so is the library here; put right,
        // it is read again.
        std::fs::write(&path, "0,1,2\tHalf a rule\n").unwrap();
        library.pin(&gun);
        assert!(library.library.trouble().is_some_and(|said| said.contains("line 1")));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "0,1,2\tHalf a rule\n");
        assert_eq!(library.label(&gun), "A gun");
        file.write(&path).unwrap();
        assert!(library.read_again());
        assert_eq!((library.library.trouble(), &*library.library), (None, &file));
        let _ = std::fs::remove_dir_all(&folder);
    }
}
