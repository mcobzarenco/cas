//! Draws the figures of the README into `docs/`. They are computed with the rules and the
//! stepping of this crate, so a picture cannot show what the automata do not do.
//!
//! ```sh
//! cargo run --release -p cas-core --example figures
//! ```

use std::{fmt::Write as _, fs, path::Path};

use cas_core::{pattern::from_rle, rules::BlockRule, universe::Universe};

// The colours of the app.
const PAPER: &str = "#1f1f24";
const DEAD: &str = "#0e0f14";
const ALIVE: &str = "#ffc46b";
const LINE: &str = "#2c2d37";
const FRAME: &str = "#3d3e4a";
/// The rule's colour: the blocks a step rewrites.
const BLOCK: &str = "#d1528b";
const TEXT: &str = "#e6e6ea";
const DIM: &str = "#9c9ca8";

const SANS: &str = "system-ui, -apple-system, 'Segoe UI', Helvetica, Arial, sans-serif";
const MONO: &str = "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace";

/// A picture in the making.
struct Svg {
    width: usize,
    height: usize,
    body: String,
}

impl Svg {
    fn new(width: usize, height: usize) -> Self {
        let mut svg = Self { width, height, body: String::new() };
        let _ = writeln!(svg.body, r#"<rect width="{width}" height="{height}" rx="10" fill="{PAPER}"/>"#);
        svg
    }

    fn rect(&mut self, x: usize, y: usize, width: usize, height: usize, style: &str) {
        let _ = writeln!(self.body, r#"<rect x="{x}" y="{y}" width="{width}" height="{height}" {style}/>"#);
    }

    fn line(&mut self, from: (usize, usize), to: (usize, usize), stroke: &str, width: f32) {
        let ((x1, y1), (x2, y2)) = (from, to);
        let _ = writeln!(
            self.body,
            r#"<line x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}" stroke="{stroke}" stroke-width="{width}"/>"#
        );
    }

    /// Text around `x`, with its baseline at `y`. It may hold `<tspan>`s.
    fn text(&mut self, x: usize, y: usize, size: usize, fill: &str, font: &str, text: &str) {
        let _ = writeln!(
            self.body,
            r#"<text x="{x}" y="{y}" font-family="{font}" font-size="{size}" fill="{fill}" text-anchor="middle">{text}</text>"#
        );
    }

    fn save(self, name: &str) {
        let (width, height) = (self.width, self.height);
        let svg = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {width} {height}\" width=\"{width}\" height=\"{height}\">\n{}</svg>\n",
            self.body
        );
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs").join(name);
        fs::write(&path, svg).unwrap_or_else(|error| panic!("cannot write {}: {error}", path.display()));
        println!("{}", path.display());
    }
}

/// A patch of the grid, drawn cell by cell.
struct Patch {
    /// Its top-left corner, and the side of a cell.
    x: usize,
    y: usize,
    cell: usize,
    columns: usize,
    rows: usize,
}

impl Patch {
    fn width(&self) -> usize {
        self.columns * self.cell
    }

    fn height(&self) -> usize {
        self.rows * self.cell
    }

    fn corner(&self, column: usize, row: usize) -> (usize, usize) {
        (self.x + column * self.cell, self.y + row * self.cell)
    }

    fn ground(&self, svg: &mut Svg) {
        svg.rect(self.x, self.y, self.width(), self.height(), &format!(r#"fill="{DEAD}""#));
    }

    fn cells(&self, svg: &mut Svg, alive: impl Fn(usize, usize) -> bool) {
        for (column, row) in (0..self.rows).flat_map(|row| (0..self.columns).map(move |column| (column, row))) {
            if alive(column, row) {
                let (x, y) = self.corner(column, row);
                svg.rect(x, y, self.cell, self.cell, &format!(r#"fill="{ALIVE}""#));
            }
        }
    }

    /// The lines between the cells and, drawn over them, the blocks of a step: those whose
    /// corners are `offset` cells from the corner of the patch, both ways.
    fn lines(&self, svg: &mut Svg, offset: Option<usize>) {
        let (right, bottom) = (self.x + self.width(), self.y + self.height());
        let around_blocks = |i: usize| offset.is_some_and(|offset| (i + offset).is_multiple_of(2));
        for blocks in [false, true] {
            let (stroke, width) = if blocks { (BLOCK, 1.5) } else { (LINE, 1.0) };
            for column in (0..=self.columns).filter(|&column| around_blocks(column) == blocks) {
                let x = self.x + column * self.cell;
                svg.line((x, self.y), (x, bottom), stroke, width);
            }
            for row in (0..=self.rows).filter(|&row| around_blocks(row) == blocks) {
                let y = self.y + row * self.cell;
                svg.line((self.x, y), (right, y), stroke, width);
            }
        }
    }

    /// Marks the block that the cell is in for a step whose blocks are offset so.
    fn tint_block(&self, svg: &mut Svg, (column, row): (usize, usize), offset: usize) {
        let first = |i: usize| i - (i + offset) % 2;
        let (x, y) = self.corner(first(column), first(row));
        svg.rect(x, y, 2 * self.cell, 2 * self.cell, &format!(r#"fill="{BLOCK}" fill-opacity="0.32""#));
    }
}

fn rule(name: &str) -> BlockRule {
    name.parse().expect("a preset")
}

/// A universe with a pattern in it, its corner at `at`.
fn world(width: usize, height: usize, rule: &BlockRule, rle: &str, at: (usize, usize)) -> Universe {
    let mut universe = Universe::new(width, height, rule.clone());
    for (x, y) in from_rle(rle).expect("a pattern") {
        universe.set(at.0 + x as usize, at.1 + y as usize, true);
    }
    universe
}

/// The live cells, in reading order.
fn living(universe: &Universe) -> Vec<(usize, usize)> {
    let cells = (0..universe.height).flat_map(|y| (0..universe.width).map(move |x| (x, y)));
    cells.filter(|&(x, y)| universe.get(x, y)).collect()
}

/// The blocks of even and of odd generations, and one cell that is in a different block
/// with different neighbours each time.
fn partition() {
    let mut svg = Svg::new(620, 316);
    let cell = (3, 4);
    for (offset, x, caption) in [(0, 30, "even generations"), (1, 350, "odd generations")] {
        let patch = Patch { x, y: 28, cell: 30, columns: 8, rows: 8 };
        patch.ground(&mut svg);
        patch.tint_block(&mut svg, cell, offset);
        patch.cells(&mut svg, |column, row| (column, row) == cell);
        patch.lines(&mut svg, Some(offset));
        svg.text(x + patch.width() / 2, 296, 15, DIM, SANS, caption);
    }
    svg.save("partition.svg");
}

/// A rule as its sixteen cases, in the order of its text form.
fn table() {
    let rule = rule("single-rotation");
    let mut svg = Svg::new(890, 262);
    let block = |svg: &mut Svg, x: usize, y: usize, state: u8| {
        svg.rect(x, y, 35, 35, &format!(r#"rx="3" fill="{FRAME}""#));
        for corner in 0..4 {
            let fill = if state >> corner & 1 == 1 { ALIVE } else { DEAD };
            svg.rect(x + 2 + 16 * (corner % 2), y + 2 + 16 * (corner / 2), 15, 15, &format!(r#"fill="{fill}""#));
        }
    };
    for state in 0..16u8 {
        let outcome = rule.table()[state as usize];
        let (x, y) = (24 + 106 * (state as usize % 8), 24 + 92 * (state as usize / 8));
        if outcome != state {
            svg.rect(x, y, 100, 78, r##"rx="6" fill="#2b2b33""##);
        }
        block(&mut svg, x + 7, y + 10, state);
        svg.text(x + 50, y + 33, 15, DIM, SANS, "→");
        block(&mut svg, x + 58, y + 10, outcome);
        svg.text(x + 24, y + 66, 13, DIM, MONO, &state.to_string());
        let color = if outcome != state { ALIVE } else { DIM };
        svg.text(x + 75, y + 66, 13, color, MONO, &outcome.to_string());
    }
    // The text form: the outcomes, in the same order.
    let entry = |(state, outcome): (usize, &u8)| {
        let color = if *outcome as usize != state { ALIVE } else { TEXT };
        format!(r#"<tspan fill="{color}">{outcome}</tspan>"#)
    };
    let entries: Vec<String> = rule.table().iter().enumerate().map(entry).collect();
    svg.text(445, 236, 16, DIM, MONO, &entries.join(","));
    svg.save("rule.svg");
}

/// A lone cell under Single rotation: every block turns it clockwise, and yet it goes round
/// the other way, because every step it is in another block.
fn orbit() {
    let mut universe = world(6, 6, &rule("single-rotation"), "o", (2, 3));
    let start = living(&universe);
    let mut svg = Svg::new(864, 200);
    for generation in 0..5 {
        let patch = Patch { x: 24 + 172 * generation, y: 24, cell: 21, columns: 6, rows: 6 };
        let cells = living(&universe);
        patch.ground(&mut svg);
        patch.tint_block(&mut svg, cells[0], universe.partition_offset());
        patch.cells(&mut svg, |column, row| cells.contains(&(column, row)));
        patch.lines(&mut svg, Some(universe.partition_offset()));
        svg.text(patch.x + patch.width() / 2, 180, 15, DIM, SANS, &format!("generation {generation}"));
        if generation < 4 {
            svg.text(patch.x + patch.width() + 23, 93, 18, DIM, SANS, "→");
            universe.step(true);
        }
    }
    assert_eq!(living(&universe), start, "the orbit closes after four generations");
    svg.save("orbit.svg");
}

/// The lightest spaceship of Single rotation, flying round a small torus for ever.
fn spaceship() {
    let (columns, rows, cell) = (36, 10, 20);
    let mut universe = world(columns, rows, &rule("single-rotation"), "b2o2$b2o", (2, 2));
    let start = living(&universe);
    // Where each of its cells is, generation by generation, until it is back where it was,
    // on the same blocks.
    let mut flight: Vec<Vec<(usize, usize)>> = Vec::new();
    while flight.is_empty() || living(&universe) != start || universe.partition_offset() != 0 {
        flight.push(living(&universe));
        universe.step(true);
    }
    let seconds = flight.len() as f32 / 10.0;

    let patch = Patch { x: 24, y: 24, cell, columns, rows };
    let mut svg = Svg::new(patch.width() + 48, patch.height() + 48);
    patch.ground(&mut svg);
    for part in 0..start.len() {
        let track = |along: fn(&(usize, usize)) -> usize, from: usize| -> String {
            let places = flight.iter().map(|cells| (from + along(&cells[part]) * cell).to_string());
            places.collect::<Vec<_>>().join(";")
        };
        let animate = |attribute: &str, values: String| {
            format!(r#"<animate attributeName="{attribute}" values="{values}" calcMode="discrete" dur="{seconds}s" repeatCount="indefinite"/>"#)
        };
        let (x, y) = patch.corner(start[part].0, start[part].1);
        let _ = writeln!(
            svg.body,
            r#"<rect x="{x}" y="{y}" width="{cell}" height="{cell}" fill="{ALIVE}">{}{}</rect>"#,
            animate("x", track(|place| place.0, patch.x)),
            animate("y", track(|place| place.1, patch.y)),
        );
    }
    patch.lines(&mut svg, None);
    svg.save("spaceship.svg");
}

/// Critters, whose empty space is all dead and all alive in turns: a glider as it is, and as
/// what differs from empty space.
fn vacuum() {
    let (columns, rows, cell) = (12, 8, 12);
    let mut universe = world(columns, rows, &rule("critters"), "$bo$2bo$2bo$bo", (2, 0));
    // The cell as it is: the pattern on top of the vacuum, whose 2×2 tile is that of the
    // blocks about to be rewritten.
    let as_it_is = |universe: &Universe, x: usize, y: usize| {
        let offset = universe.partition_offset();
        let corner = (x + offset) % 2 + 2 * ((y + offset) % 2);
        universe.get(x, y) != (universe.vacuum() >> corner & 1 == 1)
    };
    let mut svg = Svg::new(864, 318);
    svg.text(432, 30, 15, DIM, SANS, "the cells");
    svg.text(432, 171, 15, DIM, SANS, "what differs from empty space");
    for generation in 0..5 {
        let x = 24 + 168 * generation;
        let (above, below) = (Patch { x, y: 44, cell, columns, rows }, Patch { x, y: 185, cell, columns, rows });
        above.ground(&mut svg);
        above.cells(&mut svg, |column, row| as_it_is(&universe, column, row));
        above.lines(&mut svg, None);
        below.ground(&mut svg);
        below.cells(&mut svg, |column, row| universe.get(column, row));
        below.lines(&mut svg, None);
        svg.text(x + above.width() / 2, 302, 14, DIM, SANS, &format!("generation {generation}"));
        universe.step(true);
    }
    svg.save("vacuum.svg");
}

fn main() {
    partition();
    table();
    orbit();
    spaceship();
    vacuum();
}
