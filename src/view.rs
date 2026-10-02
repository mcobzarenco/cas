//! Drawing the universe and looking around in it.
//!
//! The cells are uploaded as a one-byte-per-cell texture and drawn by a UI material whose
//! fragment shader (`grid.wgsl`) applies the view transform, colours the cells, optionally
//! puts the vacuum back under them (when it is not hidden) and overlays the cell grid and the
//! 2×2 block partition. Zooming, panning and the overlays therefore cost nothing on the CPU.
//!
//! Interaction goes through picking events on the grid node: the wheel zooms about the pointer,
//! right- or middle-drag pans, left-drag paints.

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

use crate::{
    sim::{Settings, SimSystems, Universe},
    ui::Aspect,
};

pub const ALIVE: Color = Color::srgb(1.0, 0.769, 0.42);
pub const DEAD: Color = Color::srgb(0.055, 0.059, 0.078);
pub const BACKGROUND: Color = Color::srgb(0.122, 0.122, 0.141);
/// The outline of the grid: the edge of the world, in the colour of the world; in that of the
/// pattern while it catches cells.
const EDGE: (Color, f32) = (Aspect::World.color(), 0.3);
const CATCHING_EDGE: (Color, f32) = (Aspect::Pattern.color(), 0.45);
/// The blocks are what the rule rewrites.
const BLOCKS: (Color, f32) = (Aspect::Rule.color(), 0.3);

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
    material: Handle<GridMaterial>,
}

/// The size of the texture that holds the cells.
fn extent(universe: &Universe) -> Extent3d {
    Extent3d {
        width: universe.width as u32,
        height: universe.height as u32,
        depth_or_array_layers: 1,
    }
}

impl FromWorld for GridAssets {
    fn from_world(world: &mut World) -> Self {
        let universe = world.resource::<Universe>();
        // One byte per cell, read with `textureLoad`, so no sampler and no filtering.
        let image = Image::new_fill(
            extent(universe),
            TextureDimension::D2,
            &[0],
            TextureFormat::R8Uint,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        );
        let image = world.resource_mut::<Assets<Image>>().add(image);
        let material = world.resource_mut::<Assets<GridMaterial>>().add(GridMaterial {
            params: GridParams::default(),
            cells: image.clone(),
        });
        Self { image, material }
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
        Self {
            center: Vec2::ZERO,
            zoom: 1.0,
            fit: true,
            zoom_range: (0.01, MAX_ZOOM),
        }
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
    /// Shift was down when the stroke began: it erases.
    erase: bool,
    /// What the stroke draws, decided by the first cell it touches.
    alive: Option<bool>,
    last: Option<IVec2>,
}

#[derive(AsBindGroup, Asset, TypePath, Debug, Clone)]
pub struct GridMaterial {
    #[uniform(0)]
    params: GridParams,
    #[texture(1, sample_type = "u_int")]
    cells: Handle<Image>,
}

impl UiMaterial for GridMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://cas/grid.wgsl".into()
    }
}

/// Mirrors `GridParams` in `grid.wgsl`, field for field.
#[derive(ShaderType, Debug, Clone, Copy, Default, PartialEq)]
struct GridParams {
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
            .add_observer(attach_material)
            .add_systems(Update, upload_cells.in_set(SimSystems::Present))
            // After layout, so that the fit follows this frame's viewport, and before asset
            // changes are collected for rendering.
            .add_systems(
                PostUpdate,
                (constrain_view, update_material)
                    .chain()
                    .after(UiSystems::Layout)
                    .before(AssetEventSystems),
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
        on(on_scroll)
    }
}

fn attach_material(add: On<Add, GridView>, assets: Res<GridAssets>, mut commands: Commands) {
    commands
        .entity(add.entity)
        .insert(MaterialNode(assets.material.clone()));
}

/// The cell bytes are the texture: a plain copy whenever the universe changed, into a texture
/// of another size when the grid has been resized.
fn upload_cells(
    universe: Res<Universe>,
    assets: Res<GridAssets>,
    mut images: ResMut<Assets<Image>>,
) {
    if !universe.is_changed() {
        return;
    }
    let Some(mut image) = images.get_mut(&assets.image) else {
        return;
    };
    let image = &mut *image;
    match image.data.as_mut() {
        Some(data) if image.texture_descriptor.size == extent(&universe) => {
            data.copy_from_slice(universe.cells());
        }
        _ => {
            image.texture_descriptor.size = extent(&universe);
            image.data = Some(universe.cells().to_vec());
        }
    }
}

/// Keeps the view fitted while `fit` is set, and within bounds otherwise.
fn constrain_view(
    mut view: ResMut<ViewState>,
    node: Single<&ComputedNode, With<GridView>>,
    universe: Res<Universe>,
) {
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
        (
            view.center.clamp(Vec2::ZERO, grid),
            view.zoom.clamp(zoom_range.0, zoom_range.1),
        )
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
    assets: Res<GridAssets>,
    node: Single<&ComputedNode, With<GridView>>,
    mut materials: ResMut<Assets<GridMaterial>>,
) {
    let pixel_ratio = 1.0 / node.inverse_scale_factor;
    let params = GridParams {
        center: view.center,
        grid_size: Vec2::new(universe.width as f32, universe.height as f32),
        scale: view.zoom * pixel_ratio,
        pixel_ratio,
        line_width: (0.75 * pixel_ratio).max(1.0),
        // Outline this generation's partition: the blocks a forward step rewrites next.
        block_offset: universe.partition_offset() as f32,
        // The stored cells are the picture without the vacuum; unhidden, the shader adds it.
        vacuum: if settings.hide_vacuum { 0 } else { universe.vacuum() as u32 },
        grid_alpha: if settings.show_grid { fade(GRID_FADE, view.zoom) } else { 0.0 },
        block_alpha: if settings.show_blocks { fade(BLOCK_FADE, view.zoom) } else { 0.0 },
        alive: linear(ALIVE, 1.0),
        dead: linear(DEAD, 1.0),
        background: linear(BACKGROUND, 1.0),
        grid_color: linear(Color::WHITE, 0.07),
        block_color: linear(BLOCKS.0, BLOCKS.1),
        edge_color: {
            let (color, alpha) = if universe.catching { CATCHING_EDGE } else { EDGE };
            linear(color, alpha)
        },
    };
    // Writing to the asset re-prepares its bind group; reading it does not.
    if let Some(mut material) = materials.get_mut(&assets.material)
        && material.params != params
    {
        material.params = params;
    }
}

fn linear(color: Color, alpha: f32) -> Vec4 {
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
    let inside = cell.x >= 0
        && cell.y >= 0
        && (cell.x as usize) < universe.width
        && (cell.y as usize) < universe.height;
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
}

impl Canvas<'_, '_> {
    /// Continues the stroke to the cell under the pointer. The first cell it touches decides
    /// what it draws: the opposite of what is there, or nothing but dead cells when erasing.
    fn stroke_to(&mut self, grid: Entity, pointer: Vec2) {
        let Ok((node, transform)) = self.nodes.get(grid) else {
            return;
        };
        let offset = offset_in(node, transform, pointer);
        let cell = (self.view.center + offset / self.view.zoom).floor().as_ivec2();
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

/// A left press starts a stroke; shift makes it an eraser.
fn on_press(press: On<Pointer<Press>>, keys: Res<ButtonInput<KeyCode>>, mut canvas: Canvas) {
    if press.button != PointerButton::Primary {
        return;
    }
    *canvas.stroke = Stroke {
        erase: keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]),
        ..default()
    };
    canvas.stroke_to(press.entity, press.pointer_location.position);
}

/// Left-drag continues the stroke; right- or middle-drag pans.
fn on_drag(drag: On<Pointer<Drag>>, mut canvas: Canvas) {
    if drag.button == PointerButton::Primary {
        canvas.stroke_to(drag.entity, drag.pointer_location.position);
    } else {
        let zoom = canvas.view.zoom;
        canvas.view.center -= drag.delta / zoom;
        canvas.view.fit = false;
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
        let mut view = ViewState {
            center: Vec2::new(100.0, 80.0),
            zoom: 4.0,
            ..default()
        };
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
        let mut view = ViewState {
            center: Vec2::new(100.0, 80.0),
            zoom: 2.0,
            zoom_range: (2.0, 40.0),
            ..default()
        };
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
