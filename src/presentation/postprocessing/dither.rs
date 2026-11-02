//! Per-layer installed System.Random1234 sequence for Dithering.Render.
//! Calls follow that layer's render operations, never elapsed wall time.
#[derive(Clone)]
pub struct DitherSequence {
    seeds: [i32; 56],
    next: usize,
    next_pair: usize,
    noise_index: usize,
}
impl Default for DitherSequence {
    fn default() -> Self {
        let mut seeds = [0_i32; 56];
        let mut mj = 161803398 - 1234;
        let mut mk = 1;
        let mut ii = 0;
        seeds[55] = mj;
        for _ in 1..55 {
            ii += 21;
            if ii >= 55 {
                ii -= 55
            }
            seeds[ii] = mk;
            mk = mj - mk;
            if mk < 0 {
                mk += i32::MAX
            }
            mj = seeds[ii];
        }
        for _ in 0..4 {
            for i in 1..56 {
                let mut j = i + 30;
                if j >= 55 {
                    j -= 55
                }
                seeds[i] -= seeds[1 + j];
                if seeds[i] < 0 {
                    seeds[i] += i32::MAX
                }
            }
        }
        Self {
            seeds,
            next: 0,
            next_pair: 21,
            noise_index: 0,
        }
    }
}
impl DitherSequence {
    pub fn noise_index(&self) -> u32 {
        self.noise_index as u32
    }
    fn sample(&mut self) -> f32 {
        let mut a = self.next + 1;
        if a >= 56 {
            a = 1
        }
        let mut b = self.next_pair + 1;
        if b >= 56 {
            b = 1
        }
        let mut value = self.seeds[a] - self.seeds[b];
        if value == i32::MAX {
            value -= 1
        }
        if value < 0 {
            value += i32::MAX
        }
        self.seeds[a] = value;
        self.next = a;
        self.next_pair = b;
        // Exact installed double literal; source then converts each NextDouble to float.
        (f64::from(value) * f64::from_bits(0x3e00000000200000)) as f32
    }
    pub fn render(&mut self, extent: [u32; 2]) -> (usize, [f32; 4]) {
        self.noise_index += 1;
        if self.noise_index >= 64 {
            self.noise_index = 0
        }
        (
            self.noise_index,
            [
                extent[0] as f32 / 64.,
                extent[1] as f32 / 64.,
                self.sample(),
                self.sample(),
            ],
        )
    }
}
