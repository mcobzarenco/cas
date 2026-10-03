// Draws the universe. Cell states come from a one-byte-per-cell integer texture; the view
// transform, colours, the vacuum and the grid / block overlays are all done per fragment,
// so zooming, panning and toggling overlays never touch the texture.
#import bevy_ui::ui_vertex_output::UiVertexOutput

struct GridParams {
    // Cell coordinates shown at the centre of the node.
    center: vec2<f32>,
    // Grid size in cells.
    grid_size: vec2<f32>,
    // Physical pixels per cell.
    scale: f32,
    // Physical pixels per logical pixel.
    pixel_ratio: f32,
    // Width of the overlay lines in physical pixels.
    line_width: f32,
    // Partition offset (0 or 1) of the blocks to outline.
    block_offset: f32,
    // The vacuum to put back under the cells: one bit for each cell of a block of the
    // partition being outlined. 0 while the vacuum is hidden.
    vacuum: u32,
    // Overlay opacities, already faded for the zoom level (0 = hidden).
    grid_alpha: f32,
    block_alpha: f32,
    alive: vec4<f32>,
    dead: vec4<f32>,
    background: vec4<f32>,
    grid_color: vec4<f32>,
    block_color: vec4<f32>,
    // The outline of the grid.
    edge_color: vec4<f32>,
    // The band around a pattern being chosen: the cells it spans, its colour (the alpha is the
    // outline's; 0 for no band) and the opacity of its fill.
    band_min: vec2<i32>,
    band_max: vec2<i32>,
    band_color: vec4<f32>,
    band_fill: f32,
    // A pattern about to be placed, shown as a ghost: its colour, where on the grid the
    // origin of its picture is, and how many cells wide and high the picture is (none for no
    // stamp).
    stamp_color: vec4<f32>,
    stamp_origin: vec2<i32>,
    stamp_size: vec2<i32>,
};

@group(1) @binding(0) var<uniform> params: GridParams;
@group(1) @binding(1) var cells: texture_2d<u32>;
// The picture of the stamp: one byte per cell, as the cells are.
@group(1) @binding(2) var stamp: texture_2d<u32>;

// 1 if the cell containing `c` is a cell of the stamp, which may reach round the torus.
fn stamp_at(c: vec2<f32>) -> f32 {
    let grid = vec2<i32>(params.grid_size);
    let cell = ((vec2<i32>(floor(c)) - params.stamp_origin) % grid + grid) % grid;
    if any(cell >= params.stamp_size) {
        return 0.0;
    }
    return f32(textureLoad(stamp, cell, 0).r & 1u);
}

// 1 if the cell containing `c` is drawn alive, 0 otherwise; for the ghost, if it is a cell of
// the stamp.
fn alive_at(c: vec2<f32>, ghost: bool) -> f32 {
    if ghost {
        return stamp_at(c);
    }
    let last = vec2<i32>(params.grid_size) - vec2<i32>(1);
    let cell = clamp(vec2<i32>(floor(c)), vec2<i32>(0), last);
    let corner = (vec2<u32>(cell) + u32(params.block_offset)) & vec2<u32>(1u);
    let vacuum = params.vacuum >> (corner.x + 2u * corner.y);
    return f32((textureLoad(cells, cell, 0).r ^ vacuum) & 1u);
}

// Share of the pixel around cell coordinate `c` that is drawn alive, or that is the ghost's.
fn coverage_at(c: vec2<f32>, ghost: bool) -> f32 {
    // Cells per pixel along one axis.
    let footprint = 1.0 / params.scale;
    if footprint <= 1.0 {
        // Four taps on a rotated grid: smooth cell edges at fractional zooms.
        return 0.25 * (alive_at(c + footprint * vec2<f32>(-0.375, -0.125), ghost)
            + alive_at(c + footprint * vec2<f32>(0.125, -0.375), ghost)
            + alive_at(c + footprint * vec2<f32>(0.375, 0.125), ghost)
            + alive_at(c + footprint * vec2<f32>(-0.125, 0.375), ghost));
    }
    // Zoomed out, several cells share the pixel: look at each of them, up to 8×8.
    let n = min(i32(ceil(footprint)), 8);
    var alive = 0.0;
    for (var j = 0; j < n; j++) {
        for (var i = 0; i < n; i++) {
            let tap = (vec2<f32>(f32(i), f32(j)) + 0.5) / f32(n) - 0.5;
            alive += alive_at(c + tap * footprint, ghost);
        }
    }
    // A lone cell would fade away in the average, so any live cell lifts the pixel. The lift
    // sets in gradually, to keep zooming smooth.
    let lift = 0.5 * clamp(footprint - 1.0, 0.0, 1.0);
    return mix(alive / f32(n * n), min(alive, 1.0), lift);
}

// Coverage of a line `width` pixels wide whose centre is `distance` pixels away.
fn stroke(distance: f32, width: f32) -> f32 {
    return clamp(0.5 * width + 0.5 - distance, 0.0, 1.0);
}

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    // Physical pixels from the centre of the node, then cell coordinates.
    let px = (in.uv - 0.5) * in.size;
    let c = params.center + px / params.scale;

    // Distance in pixels to the grid rectangle: negative inside, positive outside.
    let q = max(-c, c - params.grid_size) * params.scale;
    let outside = max(q.x, q.y);

    var color = mix(params.dead.rgb, params.alive.rgb, coverage_at(c, false));
    // The ghost is drawn as the cells are, so that it shows as well from far away.
    if params.stamp_size.x > 0 {
        color = mix(color, params.stamp_color.rgb, params.stamp_color.a * coverage_at(c, true));
    }

    // Cell grid: a line on every integer coordinate.
    let to_cell_edge = abs(fract(c + 0.5) - 0.5) * params.scale;
    let grid = stroke(min(to_cell_edge.x, to_cell_edge.y), params.line_width);
    color = mix(color, params.grid_color.rgb, grid * params.grid_alpha * params.grid_color.a);

    // Block partition: a line on every second coordinate, shifted by the partition offset.
    let b = (c - params.block_offset) * 0.5;
    let to_block_edge = abs(fract(b + 0.5) - 0.5) * 2.0 * params.scale;
    let block = stroke(min(to_block_edge.x, to_block_edge.y), params.line_width);
    color = mix(color, params.block_color.rgb, block * params.block_alpha * params.block_color.a);

    // The band: a fill over the cells it spans, and along its edge a dark seam with the
    // outline just outside it, so that the band shows against live cells on either side.
    if params.band_color.a > 0.0 {
        let lo = vec2<f32>(params.band_min);
        let hi = vec2<f32>(params.band_max) + 1.0;
        let d = max(lo - c, c - hi) * params.scale;
        let to_band = max(d.x, d.y);
        let inside = clamp(0.5 - to_band, 0.0, 1.0);
        color = mix(color, params.band_color.rgb, inside * params.band_fill);
        let w = params.line_width;
        color = mix(color, params.background.rgb, stroke(abs(to_band), w) * params.band_color.a);
        color = mix(color, params.band_color.rgb, stroke(abs(to_band - 1.5 * w), w) * params.band_color.a);
    }

    // Outside the grid: the background, darkened by a soft shadow hugging the grid.
    let shadow = 0.5 * exp(-max(outside, 0.0) / (14.0 * params.pixel_ratio));
    let background = params.background.rgb * (1.0 - shadow);
    var out = mix(background, color, clamp(0.5 - outside, 0.0, 1.0));

    // An outline marks the edge of the grid.
    let edge = stroke(abs(outside), params.line_width);
    out = mix(out, params.edge_color.rgb, edge * params.edge_color.a);
    return vec4<f32>(out, 1.0);
}
