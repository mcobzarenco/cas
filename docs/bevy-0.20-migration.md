# Moving cas to Bevy 0.20

A scouting report and a plan, written 2026-10-10 against Bevy 0.19.1 (what cas builds with)
and the Bevy 0.20.0 release (tag `v0.20.0` in `../bevy`, the announcement at
`https://bevy.org/news/bevy-0-20`, the migration guide at
`https://bevy.org/learn/migration-guides/0-19-to-0-20/`). It is for whoever does the
migration, over several sessions: what cas uses of Bevy, what 0.20 breaks of it, what 0.20
offers that should replace something of ours, and in what order to do it so that every step
leaves a tree that builds, passes the rig and can be committed on its own.

The aim is not to get 0.20 to compile but to end up with the app written the way 0.20 is
meant to be used: upstream widgets where they do what ours do, no homebrew for what the
engine now does, the scene syntax as it is now written.

## 1. What cas uses of Bevy

Counts are of the source at the time of writing (`src/` and `crates/cas-ui/`).

| What | Where | How much |
|---|---|---|
| `bsn!` scenes | every panel and element | 120 `bsn!`, 26 `bsn_list!`, 154 `template_value(…)`, 60 `#Name`s, 58 `on(…)` observers, 31 `spawn_scene` |
| Feathers widgets | `FeathersButton` (buttons, the Play button as `Primary`), `FeathersMenu`/`MenuButton`/`MenuPopup`/`MenuItem`/`MenuDivider` (the rule menu, the grid-size menu), `FeathersTextInput`/`TextInputContainer` (the rule field, the library's fields, the pattern text), `FeathersScrollbar` (inside the kit's `scrolling()`) | 21 `@Feathers…` scene components |
| Headless widgets (`bevy_ui_widgets`) | `Checkbox`, `Slider` with `SliderValue`/`SliderDragState`/`SliderThumb`/`TrackClick`, `ScrollArea`, `Scrollbar`, `Activate`, `ActivateOnPress`, `ValueChange<bool>`/`<f32>`, `MenuItem` | the kit's `controls.rs`, `ui.rs` |
| Theme | `create_dark_theme()` extended with eight token colours (`aspect.rs`): the Play button in the time colour, the text field's cursor and selection in the rule colour, the focus ring and the scrollbar thumb in greys; `ThemeBackgroundColor`, `ThemeTextColor`, `ThemedText`, `tokens::*`, `palette::*`, `constants::fonts::{REGULAR, BOLD, MONO}` | kit and app |
| Picking | `Hovered`, `Pickable::IGNORE`, `Pointer<Click>` ×18, `Pointer<Scroll>` ×2, `Pointer<Press/Release/Move/Drag/DragEnd/Out>` ×1 each (the grid: painting, panning, the stamp), `click.propagate(false)`, `PickingSystems` (the rig) | app, kit |
| Cursor | `feathers::cursor::EntityCursor::System(SystemCursorIcon::Pointer)` | kit (`controls.rs`, `lists.rs`), `sampler.rs`, `editor.rs`, `analysis.rs`, `view.rs` |
| Text | `TextFont`/`FontSource::Handle`/`FontSourceTemplate::Handle`, `FontWeight`, `LetterSpacing`, `TextLayout { justify, linebreak }`, `EditableText` (`value()`, `queue_edit(TextEdit::SelectAll/Insert/TextStart)`), `TextEditChange` | app, kit |
| Input focus | `InputFocus`, `FocusCause::Navigated`, `FocusedInput<KeyboardInput>`; the `bevy_input_focus::tab_navigation=error` log filter (`main.rs`) | `actions.rs`, `library.rs`, fields |
| Lifecycle observers | `On<Add, GridView>`, `On<Add, SmallView>` (the material node), `On<Add, PatternText>`, `On<Add, RuleStringInput>` (a monospace `TextFont` over the feathers field's) | `view.rs`, `analysis.rs`, `editor.rs` |
| UI | `Node` fields incl. `border_radius: px(…)` ×27, `BorderRadius::MAX`, `UiRect`, `px`/`percent`, `Display`, `Overflow`, `ScrollPosition`, `ComputedNode` (`size`, `content_size()`, `inverse_scale_factor`), `UiTransform`/`Rot2` (turned icons, chevrons), `UiGlobalTransform`, `CalculatedClip` and `UiSystems`/`VisibilitySystems` (the kit's `turned.rs`: hides turned nodes that scrolled out, since 0.19 did not clip them) | everywhere |
| Rendering | a `UiMaterial` (`GridMaterial`, `AsBindGroup` with a uniform and two `u_int` textures) on a `MaterialNode`, `UiMaterialPlugin`, the fragment shader `src/grid.wgsl` with `#import bevy_ui::ui_vertex_output::UiVertexOutput`, `embedded_asset!`; `EmbeddedAssetRegistry` for the icon font; `Screenshot::primary_window()` + `save_to_disk` (the rig) | `view.rs`, `analysis.rs`, `icons.rs`, `rig.rs` |
| ECS and app | `Single`, `Local`, `resource_changed`/`or_else`/`or_eager`, `set_if_neq`, `bypass_change_detection`, `set_changed`, `Has<Checked>`, `despawn_related::<Children>`, `add_children`, `iter_descendants`, `ChildOf::parent`, `commands.trigger`, `#[derive(SystemParam)]`, `MessageWriter<AppExit>`/`Messages`/`MessageUpdateSystems` (the rig injects `KeyboardInput`, `MouseButtonInput`, `MouseWheel`, `CursorMoved`), `AsyncComputeTaskPool`/`Task`/`check_ready`, `platform::time::Instant`, `LogPlugin` with `DEFAULT_FILTER`, `Clipboard` (`set_text`, `fetch_text().poll_result()`) | app |
| Cargo features | `default_app std multi_threaded bevy_winit x11 wayland ui_bevy_render bevy_picking ui_picking scene default_font png bevy_feathers bevy_clipboard system_clipboard`; `dev = dynamic_linking`; the kit: `std bevy_feathers bevy_scene bevy_picking` | `Cargo.toml`, `crates/cas-ui/Cargo.toml` |

Things that are ours and exist because 0.19 had no upstream equivalent, or did not do it
right: the kit's tabs (`tab`, `tab_bar`, `style_tabs`), the clipping of turned nodes
(`turned.rs`, and the eight arrow glyphs of a dial where one turned glyph would do), the
scrolling of the library's list to the row on the grid (`reveal` in `library.rs`, doing the
arithmetic itself), the chips, the outlined checkbox and the slider's own looks (on the
headless widgets: these stay ours by design), the virtual list of kept patterns (rows only
for what is in sight, `kept.rs`), the rebuilding of the rule menu's items on every change
of the library (`list_rule_menu`).

## 2. What 0.20 breaks, and what to do about it

All of these are in the migration guide; the paths below say where it hits cas.

| Change | Where in cas | What to do | Size |
|---|---|---|---|
| **BSN syntax**: every scene reference takes `@` (`@sans(…)`, `@list_row()`, `@{ expr }`); `template_value(x)` is deprecated and `x` is written bare; enums with `Default + Clone` are written bare too, every field given; entities in `Children [ … ]` and `bsn_list!` are separated by `--`, with `( … )` and `,` deprecated; `bsn_list! { }` with braces, not brackets | every `bsn!` in `src/` and the kit, the gallery | A mechanical sweep, crate by crate: the kit first (it has no app code), then the app panel by panel. The `{ chips }` and `{ orbits }` expressions in lists are scene lists and keep their braces without `@`; single scene values get `@`. Every `Finding::Hex`, `Part::Espca`, `When::Listed`, `Says::Status`, `Field::Filter`, `Does(…)`, `Pickable::IGNORE`, `Name` value loses its wrapper. `Lamp::Flow { before, after }` already names every field. rustfmt will not touch `bsn_list! { }`. | the largest single piece of work: 154 + 120 + 26 sites, a few hours, all of it verified by screenshots |
| **Observers lose the `B: Bundle` generic**: `On<Add, X>` → `On<Add<X>>` | 4 sites (`view.rs:461`, `analysis.rs:472, 479`, `editor.rs:1051`) | rename | minutes |
| **Flat pointer events**: `Pointer<Click>` → `PointerClick`, `Pointer<Scroll>` → `PointerScroll`, and so on; `pointer_location.position` → `pointer.position` (a `Vec2`, `pointer.location()` for a `Location`) | 26 sites; `view.rs` reads `press.pointer_location.position` and `drag.pointer_location.position` | rename the types, `click.button`, `click.entity`, `scroll.y`, `scroll.unit` and `On::propagate` stay as they are | an hour |
| **`cursor` moved** from `bevy_feathers::cursor` to `bevy_picking::cursor` | 6 files | change the import | minutes |
| **Contextual theming**: `ThemeProps` is `{ token_assignments: ThemeToken → SemanticToken, semantic_base: SemanticToken → Color, semantic_overrides: SurfaceLevel → … }`; `create_dark_theme()` returns that | `crates/cas-ui/src/aspect.rs` (`theme()` extends `theme.color`) | Make semantic tokens of our own (`cas.fill.time`, `cas.fill.time.hover`, `cas.fill.time.pressed`, `cas.fill.rule`, `cas.fill.rule.cursor`, `cas.focus.ring`, `cas.scrollbar.thumb`, …), give them our colours in `semantic_base`, and point the feathers theme tokens we used to override (`BUTTON_PRIMARY_BG` and its hover and pressed, `TEXT_INPUT_CURSOR`, `TEXT_INPUT_SELECTION` if it still exists, `FOCUS_RING`, `SCROLLBAR_THUMB`, `SCROLLBAR_THUMB_HOVER`) at them in `token_assignments`. Do not recolour feathers' own `fill.accent.*` tokens: that would spread one aspect's colour over everything accented. | an hour, then look at every widget |
| **WESL shaders**: the naga_oil dialect is gone; `.wgsl` files with directives must become `.wesl` | `src/grid.wgsl` (one `#import`, no `#ifdef`), `view.rs` (`embedded_asset!(app, "grid.wgsl")`, `"embedded://cas/grid.wgsl"`) | Rename to `grid.wesl`; first line `import bevy_ui_render::ui_vertex_output::UiVertexOutput;` (the module is the crate's path now, see `assets/shaders/custom_ui_material.wesl` in `../bevy`, which also shows `@group(1)` as ours is); update the two paths and the doc comments. The bind group is unchanged. | half an hour, then every screenshot of the grid |
| **`EditableText` and `TextInput` are separate** | we only use `@FeathersTextInput {}` scenes, which carry both | nothing, but read the new `TextInput` docs for the shortcut semantics | — |
| **`Escape` in a text field now clears the focus and propagates** | `actions.rs` (the keyboard, "an Escape that leaves a text field is that field's"), every rig script that types then presses Escape | Our handler runs on the same press that blurred the field. Decide: keep two-step (test `input.original_event_target()` against `EditableText` as the guide shows) or accept one press. The rig scripts say what happens today (`rules.cas` "Escape leaves the field as well", `library.cas`, `kept.cas`, `analysis.cas`): run them and read the failures. | an hour |
| **`BorderRadius` fields are `CornerRadius`** | `border_radius: px(5)` ×27, `BorderRadius::MAX` | 0.20 has `impl<T: Into<CornerRadius>> From<T> for BorderRadius` and `BorderRadius::MAX`, so this most likely compiles as is; fix what does not with `.into()` | minutes |
| **`FontSource` generic families** are constructors now | we use `FontSource::Handle` and `FontSourceTemplate::Handle` only | nothing | — |
| **`TextFont::default()` is `rem(1)`** | every text of ours sets its size; feathers' texts are feathers' | nothing; `RemSize` stays 20 | — |
| **`CalculatedClip` is an enum** (`Rects(…)` with a `world_to_clip_local: Affine2` each, or `FullyClipped`) | `crates/cas-ui/src/turned.rs` reads it | see §3: the workaround should go, not be ported | — |
| **Built-in schedules order sets weakly** (`UiSystems` among them) | `turned.rs` runs in `UiSystems::PostLayout`; `view.rs` reads `ComputedNode`s after layout | they communicate through components, so the order holds; nothing to do, but keep it in mind if a frame looks one step behind | — |
| **`Name` from `&str` wants `'static`** | `Name::new(format!(…))` is a `String`, literals are static | nothing | — |
| **`FocusCause::Auto`** is new | we never match on it | nothing | — |
| **Panics in systems become errors** (re-panic by default) | the rig counts on a panic ending the run with a non-zero status | unchanged by default; do not install a swallowing handler | — |
| `ui::Interaction` deprecated | not used (we use `Hovered` and `Pressed`) | — | — |
| **Cargo**: `custom_cursor` lives on `bevy_picking`; `scene = bevy_world_serialization + bevy_scene` | `Cargo.toml` | bump to `0.20`; the app can ask for `bevy_scene` instead of `scene` (it serialises no worlds); keep the rest; check `bevy_input_focus` and `bevy_window` come in through `bevy_feathers`/`bevy_winit` as before (they did in 0.19) | minutes |

Not touched by 0.20 as far as the guide and the source say: `Screenshot::primary_window`,
`save_to_disk`, `Clipboard`, `check_ready`, `Instant`, `embedded_asset!`,
`EmbeddedAssetRegistry`, `MessageWriter`/`Messages`, `UiSystems::PostLayout`,
`TextEdit::{SelectAll, Insert, TextStart}`, `Checkbox`/`Slider`/`ScrollArea`/`Scrollbar`,
the `FeathersMenu` family, `FeathersScrollbar` (it gained `ScrollbarGutter`), `@Feathers… { @prop: … }`, `#Name`, `on(…)`.

## 3. What 0.20 offers, and what of ours it replaces

In order of how sure the win is.

1. **Clipping of turned nodes, done by the engine.** 0.20 keeps each clip rect with the
   transform from world space into the clipping node's local space, so a descendant turned
   with `UiTransform` is clipped against its scroll area like any other. That is what
   `turned.rs` (57 lines, a system in `UiSystems::PostLayout` that hides turned nodes outside
   their clip) was for, and the reason a dial has eight arrow glyphs instead of one turned
   eight ways. Delete `turned.rs` and `turned::plugin`, then look: the dials of the kept
   spaceships scrolled half out of view (`kept.cas`, a scroll of `KeptSpaceshipList`), the
   chevrons of folded sections, the diagonal mirror chips (`Sign::Turned`). If anything
   leaks, keep the file and say why in a comment. The eight glyphs can stay; they cost
   nothing.

2. **Headless tabs** (`TabList`, `Tab`, `SelectedTab`, `ValueChange<Option<Entity>>`,
   `tablist_self_update`, `TabActivation`; the example `examples/ui/widgets/headless_tabs.rs`
   in `../bevy`) replace the kit's `tab`, `tab_bar` and `style_tabs` and the library's
   `TabButton`/`pick_tab`. Keep our looks: the tab is a `Tab` with our node and underline;
   `style_tabs` colours by `Has<Selected>` and `Hovered` instead of `Has<Checked>`; the
   library observes `ValueChange<Option<Entity>>` on the `TabList`, maps the entity to its
   `Tab` enum through a component on the tab, calls `show_tab`, and writes `SelectedTab`.
   Keyboard and accessibility come free. The rig reads `expect_checked CollectionTab on`
   through `Has<Checked>`: either keep inserting `Checked` on the selected tab as well, or
   teach the rig `Selected`. Update the gallery's tab bar at the same time.

3. **`ScrollIntoView`** (an `EntityEvent` on a row, propagating to its scrollable parent)
   replaces the arithmetic of `reveal` in `library.rs` (the list scrolled to the row of the
   rule on the grid after a step with the arrow keys): trigger it on the current row and
   drop the physical-pixel maths. Check it respects the row kept in sight either side that
   `reveal` adds; if not, keep `reveal`. The kept lists do not scroll to a row, and their
   rows are a window, so nothing changes there.

4. **`Ready`** (triggered for each entity of a spawned scene once its whole hierarchy is
   there) is the right hook where we react to a feathers scene after it is spawned: the
   monospace `TextFont` put over the feathers field's own (`use_monospace`, `text_in_mono`)
   is an `On<Add<…>>` today and works; `Ready` would be the idiomatic form if any of them
   ever needs the field's children. Low priority; mention it where the observers are.

5. **`FeathersNumberInput`** (now scrubbable, with `HardLimit`, `SoftLimit`,
   `NumberInputStep`, `NumberInputPrecision`, the value set by inserting `NumberInputValue`)
   can replace the `−`/`+` chip buttons next to the sparse chip in the rule properties
   (`sampler.rs`, `SparseLess`/`SparseMore`): the chip stays the switch, a small number
   input holds the count (hard limit 2…16, step 1). Rig names and `library.cas` change with
   it. Worth it if it looks right in the chip row; a session of its own.

6. **`FeathersSelect`** (a dropdown of string options with `OptionIndex` and
   `ValueChange<Entity>`) could replace the grid-size `FeathersMenu` in `ui.rs`. The rule
   menu stays a `FeathersMenu`: it has headings, dividers and items that change with the
   library. **`FeathersLazyMenu`** builds its popup when opened from an
   `Arc<dyn Fn() -> Box<dyn Scene>>`, which cannot read the `RuleLibrary` resource, so it
   does not fit the rule menu; `list_rule_menu` keeps rebuilding the items on a change of
   the library's revision.

7. **`ThemeContext(SurfaceLevel::…)`** on our cards (the panel card, the inner cards on
   `GRAY_2`) would let feathers widgets inside them pick contextual colours (the text fields
   in the "on the grid" card, menus). Try it once the theme is ported; keep it only if the
   fields look better for it.

8. **`FeathersListView` / `ListBox` + `ListItem`** (a selectable list with keyboard
   navigation through `ActiveDescendant`, selection as `ValueChange<Entity>`) is what the
   rule library's list is, in spirit: rows, one of them the rule on the grid, the arrow keys
   going through them. Our rows carry buttons and a second line, the arrow keys work while
   the panel is open without the list having focus, and the kept lists are windowed; so
   this is an evaluation, not a plan: build the library's list on `ListBox`/`ListItem` in a
   branch, see whether the keyboard model (focus on the list) is acceptable, and keep it
   only if it is at least as good. The virtual window of `kept.rs` stays ours either way;
   0.20 has nothing like it.

9. **`Val::Em` / `Val::Rem`** make a UI scale with `RemSize`. Our sizes are all `px`. A
   later feature, not a migration item.

10. **`chain_weak`**, **`despawn_all`**, **per-column change ticks**, `FixedNode`,
    `InlineImage`, `BevyError::context`: nothing of ours needs them now. UI rendering being
    retained is a free win for the panels with many nodes.

## 4. The end state

- Bevy 0.20 from crates.io, features as in §2, no deprecated items (`template_value`,
  `bsn_list![]`, `(…)`/`,` in lists) and `cargo clippy -D warnings` clean.
- Every scene in the 0.20 syntax; scene functions and includes with `@`, entities with `--`.
- `PointerClick` and friends; `On<Add<…>>`; `bevy::picking::cursor::EntityCursor`.
- The theme as semantic tokens of ours mapped from feathers' theme tokens.
- `src/grid.wesl`.
- Tabs on `TabList`/`Tab`/`SelectedTab`; no `turned.rs`; `reveal` on `ScrollIntoView`
  if it serves.
- Every rig script and the tour passing; the README pictures remade if anything moved by a
  pixel that matters (`docs/tour.cas` has the recipe); DEVELOPMENT.md's Bevy section and
  node names updated; the gallery showing every element of the kit as it is.

## 5. The order of work

Each step ends with `cargo fmt`, `cargo clippy --release -j 4 --workspace --all-targets -- -D warnings`,
the tests, every rig script (`rig/*.cas`) and `docs/tour.cas`, and the screenshots read, not
counted; then one signed commit. Build with `-j 4` (more crashes rustc on this machine).
Take a baseline of the rig screenshots with the 0.19 binary first (`rig-run-all.sh` and
`rig-compare.py`, kept with the memory notes): most screenshots are the same pixels run to
run, and every difference after the move must be explained (a theme colour, a widget's
padding) or fixed.

**Session 1, the move itself.** Bump the workspace to 0.20 and fix what does not compile,
in this order, committing each: Cargo features; imports (`cursor`, observers, pointer
events); the theme (`aspect.rs`); the shader; then the BSN sweep, the kit first and the
gallery with it, then the app one file at a time (`ui.rs`, `editor.rs`, `library.rs`,
`sampler.rs`, `kept.rs`, `catcher.rs`, `analysis.rs`, `view.rs`). Behaviour identical to
0.19; the Escape change decided and the rig scripts adjusted or the guard added. Expect the
sweep to take most of the session.

**Session 2, what the engine does now.** Delete `turned.rs` (§3.1) and look at the dials and
chevrons; tabs on the headless widgets (§3.2) with the gallery and the rig; `ScrollIntoView`
for `reveal` (§3.3). Each its own commit, each verified by the screenshots it touches.

**Session 3, optional adoptions.** The number input for the sparse count (§3.5), the select
for the grid size (§3.6), `ThemeContext` on the cards (§3.7), the `ListBox` evaluation
(§3.8). Any of these can be dropped on sight.

**Always.** README and DEVELOPMENT.md for anything a user or a developer sees; the gallery
for anything the kit gains or loses; the memory notes for what was decided and why.

## 6. Risks and gotchas

- The BSN sweep is big and dull, and a slip is invisible until a screenshot shows it: a
  `template_value(name)` dropped rather than unwrapped loses a rig name; a `@` forgotten on a
  scene function makes the macro read it as a component. Do it in small commits, run the
  rig after each, and grep for `template_value(` and `bsn_list![` at the end.
- `rustfmt` reflows long lines: a script that edits by exact text must be re-checked after
  `cargo fmt` (it has bitten before).
- A background verification chain must gate on every step's own exit status, and never
  commit or push by itself: on 2026-10-10 a chain gated on the rig scripts committed a tree
  that did not compile, because the rig had run the previous binary.
- `Escape` semantics (§2) change user-visible behaviour; decide it deliberately.
- Feathers' dark theme may have shifted a shade here and there between 0.19 and 0.20
  (the semantic tokens map the same `palette` greys, but tokens were regrouped): compare the
  screenshots, not the code.
- The machine: `-j 4`; rustc has crashed in full-parallel builds; the app has segfaulted
  twice inside Bevy's ECS in earlier sessions (not reproduced).
- Scripted runs (the rig) have no library file and no patterns folder; the rig names of rows
  are absolute indices, and only the rows in sight of a kept list exist.

## 7. Where to read

- `../bevy` at `v0.20.0`: `crates/bevy_scene/macros/src/lib.rs` (the `bsn!` syntax
  reference, a table), `crates/bevy_ui_widgets/src/tabs.rs` and
  `examples/ui/widgets/headless_tabs.rs`, `crates/bevy_ui_widgets/src/scrollbar.rs`
  (`ScrollIntoView`), `crates/bevy_ui_widgets/src/list.rs` (`ListBox`, `ListItem`,
  `ActiveDescendant`), `crates/bevy_feathers/src/{theme.rs, dark_theme.rs, tokens.rs}`,
  `crates/bevy_feathers/src/controls/{listview.rs, select.rs, menu.rs, number_input.rs,
  text_input.rs, scrollbar.rs}`, `crates/bevy_picking/src/{events.rs, cursor.rs}`,
  `crates/bevy_ui/src/layout/clipping.rs`, `crates/bevy_ui_render/src/ui_vertex_output.wesl`,
  `assets/shaders/custom_ui_material.wesl` and `examples/ui/ui_material.rs`.
- The migration guide and the announcement, as published (the markdown lives in the
  `bevy-website` repository under `content/learn/migration-guides/0.19-to-0.20.md` and
  `content/news/2026-10-08-bevy-0.20/index.md`; the `release-content` folder of the engine
  repository is emptied at every release).
