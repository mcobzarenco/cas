// Draws the universe. Cell states come from a one-byte-per-cell integer texture; the view
// transform, colours, vacuum inversion and the grid / block overlays are all done per fragment,
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
    // 1 to draw the complement of the cells.
    invert: f32,
    // Overlay opacities, already faded for the zoom level (0 = hidden).
    grid_alpha: f32,
    block_alpha: f32,
    alive: vec4<f32>,
    dead: vec4<f32>,
    background: vec4<f32>,
    grid_color: vec4<f32>,
    block_color: vec4<f32>,
};

@group(1) @binding(0) var<uniform> params: GridParams;
@group(1) @binding(1) var cells: texture_2d<u32>;

// 1 if the cell containing `c` is drawn alive, 0 otherwise.
fn alive_at(c: vec2<f32>) -> f32 {
    let last = vec2<i32>(params.grid_size) - vec2<i32>(1);
    let cell = clamp(vec2<i32>(floor(c)), vec2<i32>(0), last);
    let state = f32(textureLoad(cells, cell, 0).r & 1u);
    return abs(state - params.invert);
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

    // Four taps on a rotated grid: smooth cell edges at fractional zooms, less shimmer when
    // several cells share a pixel.
    let o = 1.0 / params.scale;
    let coverage = 0.25 * (alive_at(c + o * vec2<f32>(-0.375, -0.125))
        + alive_at(c + o * vec2<f32>(0.125, -0.375))
        + alive_at(c + o * vec2<f32>(0.375, 0.125))
        + alive_at(c + o * vec2<f32>(-0.125, 0.375)));
    var color = mix(params.dead.rgb, params.alive.rgb, coverage);

    // Cell grid: a line on every integer coordinate.
    let to_cell_edge = abs(fract(c + 0.5) - 0.5) * params.scale;
    let grid = stroke(min(to_cell_edge.x, to_cell_edge.y), params.line_width);
    color = mix(color, params.grid_color.rgb, grid * params.grid_alpha * params.grid_color.a);

    // Block partition: a line on every second coordinate, shifted by the partition offset.
    let b = (c - params.block_offset) * 0.5;
    let to_block_edge = abs(fract(b + 0.5) - 0.5) * 2.0 * params.scale;
    let block = stroke(min(to_block_edge.x, to_block_edge.y), params.line_width);
    color = mix(color, params.block_color.rgb, block * params.block_alpha * params.block_color.a);

    // Outside the grid: the background, darkened by a soft shadow hugging the grid.
    let shadow = 0.5 * exp(-max(outside, 0.0) / (14.0 * params.pixel_ratio));
    let background = params.background.rgb * (1.0 - shadow);
    var out = mix(background, color, clamp(0.5 - outside, 0.0, 1.0));

    // A faint outline marks where the torus wraps.
    out = mix(out, params.grid_color.rgb, 0.09 * stroke(abs(outside), params.line_width));
    return vec4<f32>(out, 1.0);
}
