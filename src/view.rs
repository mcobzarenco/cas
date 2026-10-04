//! Drawing the universe and looking around in it.
//!
//! The cells are uploaded as a one-byte-per-cell texture and drawn by a UI material whose
//! fragment shader (`grid.wgsl`) applies the view transform, colours the cells, optionally
//! puts the vacuum back under them (when it is not hidden) and overlays the cell grid and the
//! 2×2 block partition. Zooming, panning and the overlays therefore cost nothing on the CPU.
//!
//! Interaction goes through picking events on the grid node: the wheel zooms about the pointer,
//! right- or middle-drag pans, left-drag paints. A pattern picked up from the spaceship list
//! ([`Stamp`]) follows the pointer as a ghost, snapped to the blocks, and a click puts it down.
//! While a pattern is being chosen for analysis, left-drag draws a band around it instead.
//!
//! The analysis panel draws its small world with the same material ([`GridParams::new`]).

use bevy::{
    asset::{AssetEventSystems, RenderAssetUsages, embedded_asset},
    feathers::cursor::EntityCursor,
    input::mouse::MouseScrollUnit,
    prelude::*,
    reflect::TypePath,
    render::render_resource::{AsBindGroup, Extent3d, ShaderType, TextureDimension, TextureFormat},
    shader::ShaderRef,
    ui::{UiGlobalTransform, UiSystems},
    window::SystemCursorIcon,
};

use cas_core::{pattern::Cell, rules::BlockRule, universe::Universe};
use cas_ui::{ALIVE, Aspect, BLOCKS, DEAD};

use crate::{
    analysis::Analysis,
    sim::{Settings, SimSystems},
};

pub const BACKGROUND: Color = Color::srgb(0.122, 0.122, 0.141);
/// The outline of the grid: the edge of the world, in the colour of the world; in that of the
/// pattern while it catches cells.
const EDGE: (Color, f32) = (Aspect::World.color(), 0.3);
const CATCHING_EDGE: (Color, f32) = (Aspect::Pattern.color(), 0.45);
/// A pattern about to be placed shows through at this opacity.
const GHOST: f32 = 0.55;
/// The band drawn around a pattern being chosen for analysis: its outline and its fill.
const BAND: (Color, f32, f32) = (Aspect::Pattern.color(), 0.8, 0.02);

/// Largest zoom, in logical pixels per cell (more when a tiny grid needs it to fit).
const MAX_ZOOM: f32 = 64.0;
/// How far out you can zoom, relative to the zoom at which the whole grid fits.
const MIN_ZOOM_FACTOR: f32 = 0.5;
/// Fraction of the viewport the grid fills when fitted.
const FIT_MARGIN: f32 = 0.94;
/// Zoom factor per mouse-wheel notch.
pub const WHEEL_ZOOM: f32 = 1.2;
/// Touchpads report pixels; this many make one notch.
const PIXELS_PER_NOTCH: f32 = 48.0;
/// Logical pixels per cell over which the overlays fade in: they would only be noise when
/// cells are tiny.
const GRID_FADE: (f32, f32) = (5.0, 10.0);
const BLOCK_FADE: (f32, f32) = (3.5, 8.0);

/// The node showing the cells; it fills the area next to the panels.
#[derive(Component, Default, Clone)]
pub struct GridView;

#[derive(Resource)]
struct GridAssets {
    image: Handle<Image>,
    /// The picture of the stamp, for its ghost.
    stamp: Handle<Image>,
    material: Handle<GridMaterial>,
}

/// The size of the texture that holds the cells.
fn extent(universe: &Universe) -> Extent3d {
    Extent3d { width: universe.width as u32, height: universe.height as u32, depth_or_array_layers: 1 }
}

impl FromWorld for GridAssets {
    fn from_world(world: &mut World) -> Self {
        let image = cell_image(world.resource::<Universe>());
        let image = world.resource_mut::<Assets<Image>>().add(image);
        let stamp = world.resource_mut::<Assets<Image>>().add(blank_image());
        let material = GridMaterial::new(image.clone(), stamp.clone());
        let material = world.resource_mut::<Assets<GridMaterial>>().add(material);
        Self { image, stamp, material }
    }
}

/// Where we are looking.
#[derive(Resource, Clone, Debug)]
pub struct ViewState {
    /// Cell coordinates shown at the centre of the viewport.
    pub center: Vec2,
    /// Logical pixels per cell.
    pub zoom: f32,
    /// Keep the whole grid fitted to the viewport; cleared by zooming or panning.
    pub fit: bool,
    /// The zooms the current viewport and grid allow, kept up to date by [`constrain_view`].
    zoom_range: (f32, f32),
}

impl Default for ViewState {
    fn default() -> Self {
        Self { center: Vec2::ZERO, zoom: 1.0, fit: true, zoom_range: (0.01, MAX_ZOOM) }
    }
}

impl ViewState {
    /// Multiplies the zoom by `factor` while keeping the cell under `offset` (logical pixels
    /// from the centre of the viewport) where it is. At a zoom limit nothing moves.
    pub fn zoom_about(&mut self, offset: Vec2, factor: f32) {
        let (min, max) = self.zoom_range;
        let anchor = self.center + offset / self.zoom;
        self.zoom = (self.zoom * factor).clamp(min, max);
        self.center = anchor - offset / self.zoom;
        self.fit = false;
    }
}

/// The paint stroke in progress.
#[derive(Resource, Default)]
struct Stroke {
    /// A stroke is being drawn: a press on the grid began it, and the button is still down.
    /// A drag paints only then: one that began as a band, or with a stamp, stays what it was
    /// when the band is called off or the stamp let go of on the way.
    active: bool,
    /// Shift was down when the stroke began: it erases.
    erase: bool,
    /// What the stroke draws, decided by the first cell it touches.
    alive: Option<bool>,
    last: Option<IVec2>,
    /// The right button has dragged since it was pressed: that was a pan, not a click.
    panned: bool,
}

/// A pattern picked up to be put on the grid, as often as one likes, until it is let go of.
#[derive(Resource, Default)]
pub struct Stamp {
    /// The pattern through the vacuum's cycle, one form per generation of it, each relative
    /// to a corner of the blocks the next step rewrites
    /// ([`Analyser::forms`](cas_core::pattern::Analyser::forms)). Empty while nothing is
    /// picked up.
    forms: Vec<Vec<Cell>>,
    /// The least and the greatest coordinates of each form: the corners of its bounding box.
    bounds: Vec<(IVec2, IVec2)>,
    /// The rule the pattern is a pattern of.
    rule: Option<BlockRule>,
    /// Which spaceship of the list it is, if it is one: the number of the haul and the kind's
    /// place in it.
    pub kind: Option<(u64, usize)>,
    /// The cell under the pointer, while the pointer is over the grid.
    hover: Option<IVec2>,
    /// How many patterns were picked up so far, which tells one from the one before.
    picked: u64,
}

impl Stamp {
    pub fn pick_up(&mut self, forms: Vec<Vec<Cell>>, rule: &BlockRule, kind: Option<(u64, usize)>) {
        let corners = |form: &Vec<Cell>| {
            let cells = form.iter().map(|&(x, y)| IVec2::new(x, y));
            cells.fold((IVec2::MAX, IVec2::MIN), |(least, greatest), cell| (least.min(cell), greatest.max(cell)))
        };
        self.bounds = forms.iter().map(corners).collect();
        self.forms = forms;
        self.rule = Some(rule.clone());
        self.kind = kind;
        self.picked += 1;
    }

    pub fn let_go(&mut self) {
        self.forms.clear();
        self.bounds.clear();
        self.rule = None;
        self.kind = None;
    }

    pub fn is_held(&self) -> bool {
        !self.forms.is_empty()
    }

    /// The form for the universe as it is now, and the corners of its bounding box.
    fn form(&self, universe: &Universe) -> &[Cell] {
        &self.forms[universe.phase() % self.forms.len()]
    }

    fn corners(&self, universe: &Universe) -> (IVec2, IVec2) {
        self.bounds[universe.phase() % self.bounds.len()]
    }

    /// Where the pattern goes for a pointer over `cell`: under the pointer, as near as the
    /// blocks allow. Its origin has to be a corner of the blocks the next step rewrites, or
    /// it would be another pattern.
    fn origin(&self, universe: &Universe, cell: IVec2) -> IVec2 {
        let (least, greatest) = self.corners(universe);
        let middle = (least + greatest) / 2;
        let offset = universe.partition_offset() as i32;
        let corner = |v: i32| v - ((v - offset) & 1);
        let anchor = cell - middle;
        IVec2::new(corner(anchor.x), corner(anchor.y))
    }

    /// Where the pattern would go now, and its cells: nothing while the pointer is elsewhere.
    fn placement(&self, universe: &Universe) -> Option<(IVec2, &[Cell])> {
        let cell = self.hover.filter(|_| self.is_held())?;
        Some((self.origin(universe, cell), self.form(universe)))
    }

    /// The size of the picture the ghost is drawn from: the form for the universe as it is
    /// now, from its origin to its last cell each way, and no further than the grid goes.
    fn picture_size(&self, universe: &Universe) -> IVec2 {
        let (_, greatest) = self.corners(universe);
        (greatest + 1).min(IVec2::new(universe.width as i32, universe.height as i32))
    }
}

#[derive(AsBindGroup, Asset, TypePath, Debug, Clone)]
pub struct GridMaterial {
    #[uniform(0)]
    params: GridParams,
    #[texture(1, sample_type = "u_int")]
    cells: Handle<Image>,
    /// The picture of a pattern about to be placed, a byte per cell as the cells are. A list
    /// of cells among the parameters would do for a small pattern only.
    #[texture(2, sample_type = "u_int")]
    stamp: Handle<Image>,
}

impl GridMaterial {
    pub fn new(cells: Handle<Image>, stamp: Handle<Image>) -> Self {
        Self { params: GridParams::default(), cells, stamp }
    }

    /// Sets the parameters, which re-prepares the bind group only if they changed.
    pub fn set(materials: &mut Assets<GridMaterial>, handle: &Handle<GridMaterial>, params: GridParams) {
        if let Some(mut material) = materials.get_mut(handle)
            && material.params != params
        {
            material.params = params;
        }
    }
}

/// The texture of a universe's cells: one byte per cell, read with `textureLoad`, so no sampler
/// and no filtering.
pub fn cell_image(universe: &Universe) -> Image {
    byte_image(extent(universe))
}

/// A texture of one dead cell: the picture of no stamp at all.
pub fn blank_image() -> Image {
    byte_image(Extent3d { width: 1, height: 1, depth_or_array_layers: 1 })
}

fn byte_image(size: Extent3d) -> Image {
    Image::new_fill(
        size,
        TextureDimension::D2,
        &[0],
        TextureFormat::R8Uint,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    )
}

/// The cell bytes are the texture: a plain copy, into a texture of another size when the grid
/// has been resized.
pub fn upload(universe: &Universe, image: &mut Image) {
    match image.data.as_mut() {
        Some(data) if image.texture_descriptor.size == extent(universe) => {
            data.copy_from_slice(universe.cells());
        }
        _ => {
            image.texture_descriptor.size = extent(universe);
            image.data = Some(universe.cells().to_vec());
        }
    }
}

/// How a universe is looked at: the cell at the centre of the node, logical pixels per cell,
/// and physical pixels per logical one.
#[derive(Clone, Copy, Debug)]
pub struct Framing {
    pub center: Vec2,
    pub zoom: f32,
    pub pixel_ratio: f32,
}

impl Framing {
    /// The whole universe in the middle of a node of this logical size.
    pub fn fitted(universe: &Universe, size: Vec2, pixel_ratio: f32) -> Self {
        let grid = Vec2::new(universe.width as f32, universe.height as f32);
        Self { center: 0.5 * grid, zoom: FIT_MARGIN * (size / grid).min_element(), pixel_ratio }
    }
}

impl UiMaterial for GridMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://cas/grid.wgsl".into()
    }
}

/// Mirrors `GridParams` in `grid.wgsl`, field for field.
#[derive(ShaderType, Debug, Clone, Copy, Default, PartialEq)]
pub struct GridParams {
    center: Vec2,
    grid_size: Vec2,
    scale: f32,
    pixel_ratio: f32,
    line_width: f32,
    block_offset: f32,
    vacuum: u32,
    grid_alpha: f32,
    block_alpha: f32,
    alive: Vec4,
    dead: Vec4,
    background: Vec4,
    grid_color: Vec4,
    block_color: Vec4,
    edge_color: Vec4,
    /// The band around a pattern being chosen: the cells it spans, and its colour (its alpha is
    /// that of the outline; 0 for no band).
    band_min: IVec2,
    band_max: IVec2,
    band_color: Vec4,
    band_fill: f32,
    /// The ghost of the stamp: its colour, where on the grid the origin of its picture is,
    /// and how many cells wide and high the picture is (none for no stamp).
    stamp_color: Vec4,
    stamp_origin: IVec2,
    stamp_size: IVec2,
}

impl GridParams {
    /// The parameters that draw `universe` as framed, with the overlays the settings ask for.
    /// The edge of the grid is drawn in `edge`; a band is drawn if given, and the ghost of a
    /// stamp, given where its picture goes and how large that is.
    pub fn new(
        universe: &Universe,
        framing: Framing,
        settings: &Settings,
        edge: (Color, f32),
        stamp: Option<(IVec2, IVec2)>,
        band: Option<(IVec2, IVec2)>,
    ) -> Self {
        let (stamp_origin, stamp_size) = stamp.unwrap_or((IVec2::ZERO, IVec2::ZERO));
        let (band_min, band_max) = band.unwrap_or((IVec2::ZERO, IVec2::ZERO));
        Self {
            center: framing.center,
            grid_size: Vec2::new(universe.width as f32, universe.height as f32),
            scale: framing.zoom * framing.pixel_ratio,
            pixel_ratio: framing.pixel_ratio,
            line_width: (0.75 * framing.pixel_ratio).max(1.0),
            // Outline this generation's partition: the blocks a forward step rewrites next.
            block_offset: universe.partition_offset() as f32,
            // The stored cells are the picture without the vacuum; unhidden, the shader adds it.
            vacuum: if settings.hide_vacuum { 0 } else { universe.vacuum() as u32 },
            grid_alpha: if settings.show_grid { fade(GRID_FADE, framing.zoom) } else { 0.0 },
            block_alpha: if settings.show_blocks { fade(BLOCK_FADE, framing.zoom) } else { 0.0 },
            alive: linear(ALIVE, 1.0),
            dead: linear(DEAD, 1.0),
            background: linear(BACKGROUND, 1.0),
            grid_color: linear(Color::WHITE, 0.07),
            block_color: linear(BLOCKS.0, BLOCKS.1),
            edge_color: linear(edge.0, edge.1),
            band_min,
            band_max,
            band_color: linear(BAND.0, if band.is_some() { BAND.1 } else { 0.0 }),
            band_fill: BAND.2,
            stamp_color: linear(ALIVE, GHOST),
            stamp_origin,
            stamp_size,
        }
    }
}

/// Needs the [`Universe`] to exist: the cell texture takes its size.
pub struct ViewPlugin;

impl Plugin for ViewPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "grid.wgsl");
        app.add_plugins(UiMaterialPlugin::<GridMaterial>::default())
            .init_resource::<GridAssets>()
            .init_resource::<ViewState>()
            .init_resource::<Stroke>()
            .init_resource::<Stamp>()
            .add_observer(attach_material)
            .add_systems(Update, let_go_of_stamp.in_set(SimSystems::Input))
            .add_systems(Update, (upload_cells, upload_stamp).in_set(SimSystems::Present))
            // After layout, so that the fit follows this frame's viewport, and before asset
            // changes are collected for rendering.
            .add_systems(
                PostUpdate,
                (constrain_view, update_material).chain().after(UiSystems::Layout).before(AssetEventSystems),
            );
    }
}

/// The grid node; [`attach_material`] gives it its material.
pub fn grid_view() -> impl Scene {
    bsn! {
        #Grid
        Node {
            flex_grow: 1.0,
            height: percent(100),
        }
        GridView
        EntityCursor::System(SystemCursorIcon::Crosshair)
        on(on_press)
        on(on_drag)
        on(on_release)
        on(on_drag_end)
        on(on_click)
        on(on_move)
        on(on_out)
        on(on_scroll)
    }
}

fn attach_material(add: On<Add, GridView>, assets: Res<GridAssets>, mut commands: Commands) {
    commands.entity(add.entity).insert(MaterialNode(assets.material.clone()));
}

/// The texture follows the universe whenever it changed.
fn upload_cells(universe: Res<Universe>, assets: Res<GridAssets>, mut images: ResMut<Assets<Image>>) {
    if universe.is_changed()
        && let Some(mut image) = images.get_mut(&assets.image)
    {
        upload(&universe, &mut image);
    }
}

/// The picture of the stamp follows the pattern picked up, in the form it would be put down
/// in now: the vacuum's cycle moves on as the world runs.
fn upload_stamp(
    stamp: Res<Stamp>,
    universe: Res<Universe>,
    assets: Res<GridAssets>,
    mut images: ResMut<Assets<Image>>,
    mut shown: Local<Option<(u64, usize, IVec2)>>,
) {
    // Which pattern, which of its forms, and how much of it the grid has room for.
    let now =
        stamp.is_held().then(|| (stamp.picked, universe.phase() % stamp.forms.len(), stamp.picture_size(&universe)));
    if *shown == now {
        return;
    }
    *shown = now;
    let Some((.., size)) = now else {
        return;
    };
    let mut picture = vec![0; (size.x * size.y) as usize];
    for &(x, y) in stamp.form(&universe) {
        if (0..size.x).contains(&x) && (0..size.y).contains(&y) {
            picture[(y * size.x + x) as usize] = 1;
        }
    }
    if let Some(mut image) = images.get_mut(&assets.stamp) {
        image.texture_descriptor.size =
            Extent3d { width: size.x as u32, height: size.y as u32, depth_or_array_layers: 1 };
        image.data = Some(picture);
    }
}

/// Keeps the view fitted while `fit` is set, and within bounds otherwise.
fn constrain_view(mut view: ResMut<ViewState>, node: Single<&ComputedNode, With<GridView>>, universe: Res<Universe>) {
    let size = node.size * node.inverse_scale_factor;
    if size.min_element() < 1.0 {
        return;
    }
    let grid = Vec2::new(universe.width as f32, universe.height as f32);
    let fit = FIT_MARGIN * (size / grid).min_element();
    let zoom_range = (MIN_ZOOM_FACTOR * fit, MAX_ZOOM.max(fit));
    let (center, zoom) = if view.fit {
        (0.5 * grid, fit)
    } else {
        (view.center.clamp(Vec2::ZERO, grid), view.zoom.clamp(zoom_range.0, zoom_range.1))
    };
    if view.center != center || view.zoom != zoom || view.zoom_range != zoom_range {
        view.center = center;
        view.zoom = zoom;
        view.zoom_range = zoom_range;
    }
}

fn update_material(
    view: Res<ViewState>,
    settings: Res<Settings>,
    universe: Res<Universe>,
    stamp: Res<Stamp>,
    analysis: Res<Analysis>,
    assets: Res<GridAssets>,
    node: Single<&ComputedNode, With<GridView>>,
    mut materials: ResMut<Assets<GridMaterial>>,
) {
    let framing = Framing { center: view.center, zoom: view.zoom, pixel_ratio: 1.0 / node.inverse_scale_factor };
    let band = analysis.band.map(|(a, b)| (a.min(b), a.max(b)));
    let ghost = stamp.placement(&universe).map(|(origin, _)| (origin, stamp.picture_size(&universe)));
    let params = GridParams::new(&universe, framing, &settings, edge_of(&universe), ghost, band);
    GridMaterial::set(&mut materials, &assets.material, params);
}

/// The colour of a universe's edge: the world's, or the pattern's while it catches cells.
pub fn edge_of(universe: &Universe) -> (Color, f32) {
    if universe.catching { CATCHING_EDGE } else { EDGE }
}

pub fn linear(color: Color, alpha: f32) -> Vec4 {
    color.to_linear().with_alpha(alpha).to_vec4()
}

/// Smoothstep from 0 at `lo` to 1 at `hi`.
fn fade((lo, hi): (f32, f32), x: f32) -> f32 {
    let t = ((x - lo) / (hi - lo)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A pointer position (logical window coordinates) relative to the centre of a node.
fn offset_in(node: &ComputedNode, transform: &UiGlobalTransform, position: Vec2) -> Vec2 {
    position - transform.translation * node.inverse_scale_factor
}

fn in_grid(cell: IVec2, universe: &Universe) -> Option<(usize, usize)> {
    let inside =
        cell.x >= 0 && cell.y >= 0 && (cell.x as usize) < universe.width && (cell.y as usize) < universe.height;
    inside.then_some((cell.x as usize, cell.y as usize))
}

/// The cells on the segment from `a` to `b`, so fast strokes leave no gaps.
fn segment(a: IVec2, b: IVec2) -> impl Iterator<Item = IVec2> {
    let delta = b - a;
    let steps = delta.abs().max_element().max(1);
    (0..=steps).map(move |i| {
        let t = i as f32 / steps as f32;
        (a.as_vec2() + delta.as_vec2() * t).round().as_ivec2()
    })
}

/// Wheel movement in notches; positive is away from the user.
pub fn wheel_notches(scroll: &Pointer<Scroll>) -> f32 {
    match scroll.unit {
        MouseScrollUnit::Line => scroll.y,
        MouseScrollUnit::Pixel => scroll.y / PIXELS_PER_NOTCH,
    }
}

/// The grid node as something to paint on and look around in.
#[derive(bevy::ecs::system::SystemParam)]
struct Canvas<'w, 's> {
    nodes: Query<'w, 's, (&'static ComputedNode, &'static UiGlobalTransform)>,
    view: ResMut<'w, ViewState>,
    universe: ResMut<'w, Universe>,
    stroke: ResMut<'w, Stroke>,
    stamp: ResMut<'w, Stamp>,
    analysis: ResMut<'w, Analysis>,
}

impl Canvas<'_, '_> {
    /// The left button came up: the stroke is over, and a band is closed.
    fn end_gesture(&mut self) {
        if self.stroke.active {
            self.stroke.active = false;
        }
        self.finish_selection();
    }

    /// Hands the live cells inside the band over for analysis, relative to a corner of the
    /// blocks the next step rewrites, as a pattern caught at the edge would be.
    fn finish_selection(&mut self) {
        let Some((a, b)) = self.analysis.band.take() else {
            return;
        };
        let last = IVec2::new(self.universe.width as i32 - 1, self.universe.height as i32 - 1);
        // The part of the band that lies on the grid: none of it, for a band drawn beside it.
        let (min, max) = (a.min(b).max(IVec2::ZERO), a.max(b).min(last));
        let offset = self.universe.partition_offset() as i32;
        let corner = |v: i32| v - ((v - offset) & 1);
        let (x0, y0) = (corner(min.x), corner(min.y));
        let mut cells = Vec::new();
        for y in min.y..=max.y {
            for x in min.x..=max.x {
                if self.universe.get(x as usize, y as usize) {
                    cells.push((x - x0, y - y0));
                }
            }
        }
        let phase = self.universe.phase();
        self.analysis.study(cells, phase, &self.universe);
    }

    /// The cell under a pointer position (logical window coordinates) over the grid node.
    fn cell_under(&self, grid: Entity, pointer: Vec2) -> Option<IVec2> {
        let (node, transform) = self.nodes.get(grid).ok()?;
        let offset = offset_in(node, transform, pointer);
        Some((self.view.center + offset / self.view.zoom).floor().as_ivec2())
    }

    /// Puts the stamp down where it hovers. The grid is a torus, so a pattern that reaches
    /// past an edge comes round the other side.
    fn place_stamp(&mut self) {
        let Some((origin, form)) = self.stamp.placement(&self.universe) else {
            return;
        };
        let (width, height) = (self.universe.width as i32, self.universe.height as i32);
        for &(x, y) in form {
            let (x, y) = ((origin.x + x).rem_euclid(width), (origin.y + y).rem_euclid(height));
            self.universe.set(x as usize, y as usize, true);
        }
    }

    /// Continues the stroke to the cell under the pointer. The first cell it touches decides
    /// what it draws: the opposite of what is there, or nothing but dead cells when erasing.
    fn stroke_to(&mut self, grid: Entity, pointer: Vec2) {
        let Some(cell) = self.cell_under(grid, pointer) else {
            return;
        };
        let alive = match (self.stroke.alive, in_grid(cell, &self.universe)) {
            (Some(alive), _) => alive,
            (None, Some((x, y))) => !self.stroke.erase && !self.universe.get(x, y),
            // Still outside the grid: the stroke starts where it enters.
            (None, None) => return,
        };
        for point in segment(self.stroke.last.unwrap_or(cell), cell) {
            if let Some((x, y)) = in_grid(point, &self.universe)
                && self.universe.get(x, y) != alive
            {
                self.universe.set(x, y, alive);
            }
        }
        self.stroke.alive = Some(alive);
        self.stroke.last = Some(cell);
    }
}

/// A left press starts a band if a pattern is being chosen, puts the stamp down if one is
/// held, and otherwise starts a stroke; shift makes the stroke an eraser.
fn on_press(press: On<Pointer<Press>>, keys: Res<ButtonInput<KeyCode>>, mut canvas: Canvas) {
    if press.button != PointerButton::Primary {
        canvas.stroke.panned = false;
        return;
    }
    canvas.stroke.active = false;
    if canvas.analysis.selecting {
        if let Some(cell) = canvas.cell_under(press.entity, press.pointer_location.position) {
            canvas.analysis.band = Some((cell, cell));
        }
        return;
    }
    if canvas.stamp.is_held() {
        canvas.stamp.hover = canvas.cell_under(press.entity, press.pointer_location.position);
        canvas.place_stamp();
        return;
    }
    *canvas.stroke =
        Stroke { active: true, erase: keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]), ..default() };
    canvas.stroke_to(press.entity, press.pointer_location.position);
}

/// Left-drag stretches the band or continues the stroke; right- or middle-drag pans.
fn on_drag(drag: On<Pointer<Drag>>, mut canvas: Canvas) {
    if drag.button == PointerButton::Primary {
        if let Some((from, _)) = canvas.analysis.band {
            if let Some(cell) = canvas.cell_under(drag.entity, drag.pointer_location.position) {
                canvas.analysis.band = Some((from, cell));
            }
        } else if canvas.stroke.active {
            canvas.stroke_to(drag.entity, drag.pointer_location.position);
        }
    } else {
        let zoom = canvas.view.zoom;
        canvas.view.center -= drag.delta / zoom;
        canvas.view.fit = false;
        canvas.stroke.panned = true;
    }
}

/// Letting go of the left button ends the stroke and closes the band, wherever the pointer is
/// by then.
fn on_release(release: On<Pointer<Release>>, mut canvas: Canvas) {
    if release.button == PointerButton::Primary {
        canvas.end_gesture();
    }
}

fn on_drag_end(end: On<Pointer<DragEnd>>, mut canvas: Canvas) {
    if end.button == PointerButton::Primary {
        canvas.end_gesture();
    }
}

/// A right click, as opposed to a right drag, lets go of the stamp.
fn on_click(click: On<Pointer<Click>>, mut canvas: Canvas) {
    if click.button == PointerButton::Secondary && !canvas.stroke.panned && canvas.stamp.is_held() {
        canvas.stamp.let_go();
    }
}

/// The stamp follows the pointer over the grid.
fn on_move(moved: On<Pointer<Move>>, mut canvas: Canvas) {
    if canvas.stamp.is_held() {
        let cell = canvas.cell_under(moved.entity, moved.pointer_location.position);
        if canvas.stamp.hover != cell {
            canvas.stamp.hover = cell;
        }
    }
}

fn on_out(_: On<Pointer<Out>>, mut stamp: ResMut<Stamp>) {
    if stamp.hover.is_some() {
        stamp.hover = None;
    }
}

/// Escape lets go of the stamp, and so does a change of rule: the pattern was that rule's.
fn let_go_of_stamp(keys: Res<ButtonInput<KeyCode>>, universe: Res<Universe>, mut stamp: ResMut<Stamp>) {
    if stamp.is_held() && (keys.just_pressed(KeyCode::Escape) || stamp.rule.as_ref() != Some(universe.rule())) {
        stamp.let_go();
    }
}

/// The wheel zooms about the pointer.
fn on_scroll(
    scroll: On<Pointer<Scroll>>,
    nodes: Query<(&ComputedNode, &UiGlobalTransform)>,
    mut view: ResMut<ViewState>,
) {
    let Ok((node, transform)) = nodes.get(scroll.entity) else {
        return;
    };
    let notches = wheel_notches(&scroll);
    if notches != 0.0 {
        let offset = offset_in(node, transform, scroll.pointer_location.position);
        view.zoom_about(offset, WHEEL_ZOOM.powf(notches));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zooming_keeps_the_anchor_cell_in_place() {
        let mut view = ViewState { center: Vec2::new(100.0, 80.0), zoom: 4.0, ..default() };
        let offset = Vec2::new(120.0, -40.0);
        let before = view.center + offset / view.zoom;
        view.zoom_about(offset, 2.5);
        let after = view.center + offset / view.zoom;
        assert!((before - after).length() < 1e-4);
        assert_eq!(view.zoom, 10.0);
        assert!(!view.fit);
    }

    #[test]
    fn zooming_at_a_limit_leaves_the_view_alone() {
        let mut view = ViewState { center: Vec2::new(100.0, 80.0), zoom: 2.0, zoom_range: (2.0, 40.0), ..default() };
        view.zoom_about(Vec2::new(300.0, 200.0), 1.0 / WHEEL_ZOOM);
        assert_eq!((view.center, view.zoom), (Vec2::new(100.0, 80.0), 2.0));
        view.zoom = 40.0;
        view.zoom_about(Vec2::new(300.0, 200.0), WHEEL_ZOOM);
        assert_eq!((view.center, view.zoom), (Vec2::new(100.0, 80.0), 40.0));
    }

    #[test]
    fn segments_are_gapless_and_include_both_ends() {
        let cells: Vec<_> = segment(IVec2::new(2, 3), IVec2::new(9, -1)).collect();
        assert_eq!(cells.first(), Some(&IVec2::new(2, 3)));
        assert_eq!(cells.last(), Some(&IVec2::new(9, -1)));
        for pair in cells.windows(2) {
            assert!((pair[1] - pair[0]).abs().max_element() <= 1);
        }
        assert_eq!(segment(IVec2::ZERO, IVec2::ZERO).count(), 2);
    }

    #[test]
    fn overlays_fade_in_with_zoom() {
        assert_eq!(fade(GRID_FADE, 2.0), 0.0);
        assert_eq!(fade(GRID_FADE, 32.0), 1.0);
        assert!(fade(BLOCK_FADE, 5.0) > 0.0 && fade(BLOCK_FADE, 5.0) < 1.0);
    }
}
