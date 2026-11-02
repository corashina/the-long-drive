//! Loose caster grid on source X and Z. Cell tests are conservative: a skipped cell
//! holds only casters that the exact f32 source tests also reject.
use super::shadow_candidates::{Bounds, Plane};
use super::shadow_frame::CasterInput;
use std::collections::HashMap;

/// Cell side in source meters.
const CELL: f32 = 32.;
/// Bound of the relative f32 error of one caster test, with slack.
const ERROR: f64 = 1. / 262_144.;

struct Cell {
    min: [f64; 3],
    max: [f64; 3],
    rows: Vec<u32>,
}

/// Each caster row lives in the cell of its bounds center.
/// Cell bounds only grow between rebuilds, so they always hold their rows.
#[derive(Default)]
pub struct CasterGrid {
    keys: HashMap<(i32, i32), u32>,
    /// Cells in use come first. Later cells keep their storage for the next rebuild.
    cells: Vec<Cell>,
    used: usize,
    /// Cell and position in the cell of each row.
    places: Vec<(u32, u32)>,
}

fn key(bounds: Bounds) -> (i32, i32) {
    // Non-finite bounds fail validation later. The saturating cast keeps this step safe.
    (
        (bounds.center[0] / CELL).floor() as i32,
        (bounds.center[2] / CELL).floor() as i32,
    )
}
fn corners(bounds: Bounds) -> ([f64; 3], [f64; 3]) {
    let c = bounds.center.map(f64::from);
    let e = bounds.extents.map(f64::from);
    (
        std::array::from_fn(|i| c[i] - e[i]),
        std::array::from_fn(|i| c[i] + e[i]),
    )
}

impl CasterGrid {
    /// Rows in the grid.
    pub fn len(&self) -> usize {
        self.places.len()
    }
    pub fn is_empty(&self) -> bool {
        self.places.is_empty()
    }
    pub fn rebuild(&mut self, casters: &[CasterInput]) {
        self.keys.clear();
        for cell in &mut self.cells[..self.used] {
            cell.rows.clear();
        }
        self.used = 0;
        self.places.clear();
        self.places.reserve(casters.len());
        for (row, caster) in casters.iter().enumerate() {
            let place = self.insert(row as u32, caster.bounds);
            self.places.push(place);
        }
    }
    /// Call after the bounds of a row change. A new row must be the next row.
    pub fn update(&mut self, row: u32, bounds: Bounds) {
        let index = row as usize;
        if index == self.places.len() {
            let place = self.insert(row, bounds);
            self.places.push(place);
            return;
        }
        let (cell, slot) = self.places[index];
        if self.keys.get(&key(bounds)) == Some(&cell) {
            self.grow(cell as usize, bounds);
            return;
        }
        let rows = &mut self.cells[cell as usize].rows;
        rows.swap_remove(slot as usize);
        if let Some(&moved) = rows.get(slot as usize) {
            self.places[moved as usize].1 = slot;
        }
        self.places[index] = self.insert(row, bounds);
    }
    fn insert(&mut self, row: u32, bounds: Bounds) -> (u32, u32) {
        let next = self.used as u32;
        let cell = *self.keys.entry(key(bounds)).or_insert(next);
        if cell == next {
            let (min, max) = corners(bounds);
            if self.used == self.cells.len() {
                self.cells.push(Cell {
                    min,
                    max,
                    rows: Vec::new(),
                });
            } else {
                self.cells[self.used].min = min;
                self.cells[self.used].max = max;
            }
            self.used += 1;
        } else {
            self.grow(cell as usize, bounds);
        }
        let rows = &mut self.cells[cell as usize].rows;
        rows.push(row);
        (cell, rows.len() as u32 - 1)
    }
    fn grow(&mut self, cell: usize, bounds: Bounds) {
        let (min, max) = corners(bounds);
        let cell = &mut self.cells[cell];
        for i in 0..3 {
            cell.min[i] = cell.min[i].min(min[i]);
            cell.max[i] = cell.max[i].max(max[i]);
        }
    }
    fn live(&self) -> impl Iterator<Item = &Cell> {
        self.cells[..self.used]
            .iter()
            .filter(|cell| !cell.rows.is_empty())
    }
    /// Visit the rows of cells that can touch the hull of `DirectionalHull::intersects`.
    pub fn visit_hull(&self, planes: &[Plane], mut visit: impl FnMut(u32)) {
        for cell in self.live() {
            let outside = planes.iter().any(|p| {
                // The hull kernel rejects when neg . center - d exceeds |neg| . extents.
                let mut low = -f64::from(p[3]);
                let mut size = f64::from(p[3]).abs();
                for i in 0..3 {
                    let n = -f64::from(p[i]);
                    low += (n * cell.min[i]).min(n * cell.max[i]);
                    size += 2. * n.abs() * cell.min[i].abs().max(cell.max[i].abs());
                }
                low > size * ERROR
            });
            if !outside {
                cell.rows.iter().copied().for_each(&mut visit);
            }
        }
    }
    /// Visit the rows of cells that can touch the sphere of `lamp_caster`.
    pub fn visit_sphere(&self, center: [f32; 3], radius: f32, mut visit: impl FnMut(u32)) {
        let c = center.map(f64::from);
        let limit = f64::from(radius) * f64::from(radius) * (1. + 16. * ERROR);
        for cell in self.live() {
            let mut gap2 = 0.;
            for i in 0..3 {
                let gap = (cell.min[i] - c[i]).max(c[i] - cell.max[i]);
                let pad = (c[i].abs() + 2. * cell.min[i].abs().max(cell.max[i].abs())) * ERROR;
                let gap = (gap - pad).max(0.);
                gap2 += gap * gap;
            }
            if gap2 <= limit {
                cell.rows.iter().copied().for_each(&mut visit);
            }
        }
    }
}
