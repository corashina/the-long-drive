//! Source TAA jitter: PPv2 `TemporalAntialiasing.GenerateRandomOffset`.
#[derive(Default)]
pub struct TaaSequence {
    sample: u32,
}
impl TaaSequence {
    /// Pixel offset of the next frame. Eight Halton (2, 3) samples, scaled by the spread.
    pub fn advance(&mut self, spread: f32) -> [f32; 2] {
        let index = (self.sample & 1023) + 1;
        self.sample += 1;
        if self.sample >= 8 {
            self.sample = 0;
        }
        [
            (halton(index, 2) - 0.5) * spread,
            (halton(index, 3) - 0.5) * spread,
        ]
    }
}
fn halton(mut index: u32, radix: u32) -> f32 {
    let mut result = 0.;
    let mut fraction = 1. / radix as f32;
    while index > 0 {
        result += (index % radix) as f32 * fraction;
        index /= radix;
        fraction /= radix as f32;
    }
    result
}
