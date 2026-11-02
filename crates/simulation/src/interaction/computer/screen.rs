//! The C64output image: the Back fill, the Front panel, the Text grid and the Cursor block
//! that the computer camera draws. Pixels are Rgba8Unorm with linear colors. Rows run
//! bottom-up, as source texture rows do.
use super::Computer;
use crate::reference::{ComputerReference, ComputerScreenReference, ItemId};
use serde::{Deserialize, Serialize};

/// The screen state of one computer, as the renderer draws it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScreenView {
    pub item: ItemId,
    pub grid: Vec<u8>,
    pub cursor: [i32; 2],
    pub cursor_visible: bool,
}
impl ScreenView {
    pub fn of(item: ItemId, computer: &Computer) -> Self {
        Self {
            item,
            grid: computer.grid().to_vec(),
            cursor: computer.cursor(),
            cursor_visible: computer.cursor_visible(),
        }
    }
}

/// A linear color in 0..1.
fn linear(srgba: [f32; 4]) -> [f32; 4] {
    let channel = |v: f32| {
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    [
        channel(srgba[0]),
        channel(srgba[1]),
        channel(srgba[2]),
        srgba[3],
    ]
}

fn quantize(color: [f32; 4]) -> [u8; 4] {
    color.map(|v| (v.clamp(0., 1.) * 255.).round() as u8)
}

struct Canvas<'a> {
    pixels: &'a mut [u8],
    size: [i32; 2],
}

impl Canvas<'_> {
    /// The byte offset of top-down pixel (x, y), or None outside the image.
    fn at(&self, x: i32, y: i32) -> Option<usize> {
        let [w, h] = self.size;
        ((0..w).contains(&x) && (0..h).contains(&y)).then(|| ((h - 1 - y) * w + x) as usize * 4)
    }
    /// A pixel-perfect canvas rect: the corners snap to whole pixels.
    fn fill(&mut self, [x0, y0, x1, y1]: [f32; 4], color: [u8; 4]) {
        let [w, h] = self.size;
        let x = (x0.round() as i32).max(0)..(x1.round() as i32).min(w);
        for y in (y0.round() as i32).max(0)..(y1.round() as i32).min(h) {
            let start = ((h - 1 - y) * w) as usize * 4;
            for px in x.clone() {
                let i = start + px as usize * 4;
                self.pixels[i..i + 4].copy_from_slice(&color);
            }
        }
    }
}

/// Draw the whole screen of `computer` into `pixels` (size.x * size.y * 4 bytes).
pub fn draw(reference: &ComputerReference, computer: &ScreenView, pixels: &mut [u8]) {
    let s: &ComputerScreenReference = &reference.screen;
    let mut canvas = Canvas {
        pixels,
        size: s.size.map(i32::from),
    };
    let [w, h] = canvas.size.map(|v| v as f32);
    canvas.fill([0., 0., w, h], quantize(linear(s.back)));
    let [fx, fy, fw, fh] = s.front_rect;
    canvas.fill([fx, fy, fx + fw, fy + fh], quantize(linear(s.front)));
    if computer.cursor_visible {
        let [cx, cy] = computer.cursor.map(|v| v as f32);
        let x = fx + cx * reference.cursor_step[0];
        let y = fy + cy * reference.cursor_step[1];
        let [cw, ch] = s.cursor_size;
        canvas.fill([x, y, x + cw, y + ch], quantize(linear(s.cursor)));
    }
    // UpdScreenText: one line per grid row. The pixel-perfect canvas snaps the text origin.
    let text = linear(s.text);
    let g = &s.glyphs;
    let cell_width = usize::from(g.cell[0]);
    let columns = usize::from(reference.x_size);
    for (row, line) in computer.grid.chunks_exact(columns).enumerate() {
        let top = (fy.round() + row as f32 * s.line_height).round() as i32 + i32::from(g.offset[1]);
        let mut pen = fx.round() as i32 + i32::from(g.offset[0]);
        for &character in line {
            // A character outside the font draws nothing and does not advance.
            let Some(cell) = g.glyph(character) else {
                continue;
            };
            for (j, coverage) in cell.chunks_exact(cell_width).enumerate() {
                for (i, &a) in coverage.iter().enumerate() {
                    let Some(p) = canvas.at(pen + i as i32, top + j as i32).filter(|_| a != 0)
                    else {
                        continue;
                    };
                    let a = f32::from(a) / 255. * text[3];
                    let dst = &mut canvas.pixels[p..p + 4];
                    for (d, t) in dst.iter_mut().zip(text) {
                        *d = (((f32::from(*d) / 255.) * (1. - a) + t * a) * 255.).round() as u8;
                    }
                }
            }
            pen += i32::from(s.advance);
        }
    }
}
