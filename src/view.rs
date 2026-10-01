//! Drawing the universe and looking around in it.
//!
//! The cells are uploaded as a one-byte-per-cell texture and drawn by a UI material whose
//! fragment shader (`grid.wgsl`) applies the view transform, colours the cells, optionally
//! complements them (hide vacuum fluctuations) and overlays the cell grid and the 2×2 block
//! partition. Zooming, panning and the overlays therefore cost nothing on the CPU.
//!
//! Interaction goes through picking events on the grid node: the wheel zooms about the pointer,
//! right- or middle-drag pans, left-drag paints.

use bevy::{
    asset::{RenderAssetUsages, embedded_asset},
    feathers::cursor::EntityCursor,
    input::mouse::MouseScrollUnit,
    prelude::*,
    reflect::TypePath,
    render::render_resource::{AsBindGroup, Extent3d, ShaderType, TextureDimension, TextureFormat},
    shader::ShaderRef,
    ui::UiGlobalTransform,
    window::SystemCursorIcon,
};

use crate::sim::{Settings, Universe, advance};

/// Largest zoom, in logical pixels per cell.
pub const MAX_ZOOM: f32 = 64.0;
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

/// The node showing the cells; it fills the area next to the panel.
#[derive(Component, Default, Clone)]
pub struct GridView;

#[derive(Resource)]
pub struct GridAssets {
    image: Handle<Image>,
    material: Handle<GridMaterial>,
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
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            center: Vec2::ZERO,
            zoom: 1.0,
            fit: true,
        }
    }
}

impl ViewState {
    /// Multiplies the zoom by `factor` while keeping the cell under `offset` (logical pixels
    /// from the centre of the viewport) where it is.
    pub fn zoom_about(&mut self, offset: Vec2, factor: f32) {
        let anchor = self.center + offset / self.zoom;
        self.zoom = (self.zoom * factor).clamp(0.01, MAX_ZOOM);
        self.center = anchor - offset / self.zoom;
        self.fit = false;
    }
}

/// The paint stroke in progress.
#[derive(Resource, Default)]
struct Stroke {
    value: bool,
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
    invert: f32,
    grid_alpha: f32,
    block_alpha: f32,
    alive: Vec4,
    dead: Vec4,
    background: Vec4,
    grid_color: Vec4,
    block_color: Vec4,
}

pub struct ViewPlugin;

impl Plugin for ViewPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "grid.wgsl");
        app.add_plugins(UiMaterialPlugin::<GridMaterial>::default())
            .init_resource::<ViewState>()
            .init_resource::<Stroke>()
            .add_systems(Startup, setup_grid)
            .add_systems(
                Update,
                (
                    attach_material,
                    upload_cells.after(advance),
                    (constrain_view, update_material).chain().after(advance),
                ),
            );
    }
}

/// The grid node. Its material is attached by [`attach_material`], since asset handles can't be
/// passed through `bsn!` values.
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

fn setup_grid(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<GridMaterial>>,
    universe: Res<Universe>,
) {
    // One byte per cell, read with `textureLoad`, so no sampler and no filtering.
    let image = images.add(Image::new_fill(
        Extent3d {
            width: universe.width as u32,
            height: universe.height as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0],
        TextureFormat::R8Uint,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    ));
    let material = materials.add(GridMaterial {
        params: GridParams::default(),
        cells: image.clone(),
    });
    commands.insert_resource(GridAssets { image, material });
}

fn attach_material(
    assets: Res<GridAssets>,
    nodes: Query<Entity, (With<GridView>, Without<MaterialNode<GridMaterial>>)>,
    mut commands: Commands,
) {
    for entity in &nodes {
        commands
            .entity(entity)
            .insert(MaterialNode(assets.material.clone()));
    }
}

/// The cell bytes are the texture: a plain copy whenever the universe changed.
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
    match image.data.as_mut() {
        Some(data) if data.len() == universe.cells.len() => data.copy_from_slice(&universe.cells),
        _ => image.data = Some(universe.cells.clone()),
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
    let (center, zoom) = if view.fit {
        (0.5 * grid, fit)
    } else {
        (
            view.center.clamp(Vec2::ZERO, grid),
            view.zoom.clamp(MIN_ZOOM_FACTOR * fit, MAX_ZOOM.max(fit)),
        )
    };
    if view.center != center || view.zoom != zoom {
        view.center = center;
        view.zoom = zoom;
    }
}

fn update_material(
    view: Res<ViewState>,
    settings: Res<Settings>,
    universe: Res<Universe>,
    assets: Res<GridAssets>,
    node: Single<&ComputedNode, With<GridView>>,
    mut materials: ResMut<Assets<GridMaterial>>,
    mut last: Local<Option<GridParams>>,
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
        invert: (settings.hide_vacuum
            && universe.rule.vacuum_flips
            && universe.generation.rem_euclid(2) == 1) as u8 as f32,
        grid_alpha: if settings.show_grid { fade(GRID_FADE, view.zoom) } else { 0.0 },
        block_alpha: if settings.show_blocks { fade(BLOCK_FADE, view.zoom) } else { 0.0 },
        alive: linear(0xFF, 0xC4, 0x6B, 1.0),
        dead: linear(0x0E, 0x0F, 0x14, 1.0),
        background: linear(0x1F, 0x1F, 0x24, 1.0),
        grid_color: linear(0xFF, 0xFF, 0xFF, 0.07),
        block_color: linear(0x6F, 0xB1, 0xFF, 0.30),
    };
    // Touching the asset re-prepares its bind group, so only do it when something changed.
    if *last != Some(params)
        && let Some(mut material) = materials.get_mut(&assets.material)
    {
        material.params = params;
        *last = Some(params);
    }
}

fn linear(r: u8, g: u8, b: u8, alpha: f32) -> Vec4 {
    Color::srgb_u8(r, g, b)
        .to_linear()
        .with_alpha(alpha)
        .to_vec4()
}

/// Smoothstep from 0 at `lo` to 1 at `hi`.
fn fade((lo, hi): (f32, f32), x: f32) -> f32 {
    let t = ((x - lo) / (hi - lo)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The cell under a pointer position given in logical window coordinates.
fn cell_at(
    position: Vec2,
    node: &ComputedNode,
    transform: &UiGlobalTransform,
    view: &ViewState,
) -> IVec2 {
    let offset = position - transform.translation * node.inverse_scale_factor;
    (view.center + offset / view.zoom).floor().as_ivec2()
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

/// Left press starts a stroke: it paints the opposite of the cell it starts on, or erases while
/// shift is held.
fn on_press(
    press: On<Pointer<Press>>,
    nodes: Query<(&ComputedNode, &UiGlobalTransform)>,
    keys: Res<ButtonInput<KeyCode>>,
    view: Res<ViewState>,
    mut universe: ResMut<Universe>,
    mut stroke: ResMut<Stroke>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    stroke.last = None;
    let Ok((node, transform)) = nodes.get(press.entity) else {
        return;
    };
    let cell = cell_at(press.pointer_location.position, node, transform, &view);
    let Some((x, y)) = in_grid(cell, &universe) else {
        return;
    };
    let erase = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let value = !erase && !universe.get(x, y);
    if universe.get(x, y) != value {
        universe.set(x, y, value);
    }
    *stroke = Stroke {
        value,
        last: Some(cell),
    };
}

/// Left-drag continues the stroke; right- or middle-drag pans.
fn on_drag(
    drag: On<Pointer<Drag>>,
    nodes: Query<(&ComputedNode, &UiGlobalTransform)>,
    mut view: ResMut<ViewState>,
    mut universe: ResMut<Universe>,
    mut stroke: ResMut<Stroke>,
) {
    if drag.button != PointerButton::Primary {
        let zoom = view.zoom;
        view.center -= drag.delta / zoom;
        view.fit = false;
        return;
    }
    let Ok((node, transform)) = nodes.get(drag.entity) else {
        return;
    };
    let cell = cell_at(drag.pointer_location.position, node, transform, &view);
    let from = match stroke.last {
        Some(last) => last,
        None => {
            // The press landed outside the grid; the stroke starts where it enters.
            let Some((x, y)) = in_grid(cell, &universe) else {
                return;
            };
            stroke.value = !universe.get(x, y);
            cell
        }
    };
    let value = stroke.value;
    for point in segment(from, cell) {
        if let Some((x, y)) = in_grid(point, &universe)
            && universe.get(x, y) != value
        {
            universe.set(x, y, value);
        }
    }
    stroke.last = Some(cell);
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
    let notches = match scroll.unit {
        MouseScrollUnit::Line => scroll.y,
        MouseScrollUnit::Pixel => scroll.y / PIXELS_PER_NOTCH,
    };
    if notches == 0.0 {
        return;
    }
    let offset = scroll.pointer_location.position - transform.translation * node.inverse_scale_factor;
    view.zoom_about(offset, WHEEL_ZOOM.powf(notches));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zooming_keeps_the_anchor_cell_in_place() {
        let mut view = ViewState {
            center: Vec2::new(100.0, 80.0),
            zoom: 4.0,
            fit: true,
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
