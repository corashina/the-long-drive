//! Native 2D noise and managed terrain math in the frozen source generation frame.
use super::SourceRandom;

// Checked UnityPlayer lookup at RVA 0x1a6a9e0. The second half repeats these values.
const PERMUTATION: [u8; 256] = [
    151, 160, 137, 91, 90, 15, 131, 13, 201, 95, 96, 53, 194, 233, 7, 225, 140, 36, 103, 30, 69,
    142, 8, 99, 37, 240, 21, 10, 23, 190, 6, 148, 247, 120, 234, 75, 0, 26, 197, 62, 94, 252, 219,
    203, 117, 35, 11, 32, 57, 177, 33, 88, 237, 149, 56, 87, 174, 20, 125, 136, 171, 168, 68, 175,
    74, 165, 71, 134, 139, 48, 27, 166, 77, 146, 158, 231, 83, 111, 229, 122, 60, 211, 133, 230,
    220, 105, 92, 41, 55, 46, 245, 40, 244, 102, 143, 54, 65, 25, 63, 161, 1, 216, 80, 73, 209, 76,
    132, 187, 208, 89, 18, 169, 200, 196, 135, 130, 116, 188, 159, 86, 164, 100, 109, 198, 173,
    186, 3, 64, 52, 217, 226, 250, 124, 123, 5, 202, 38, 147, 118, 126, 255, 82, 85, 212, 207, 206,
    59, 227, 47, 16, 58, 17, 182, 189, 28, 42, 223, 183, 170, 213, 119, 248, 152, 2, 44, 154, 163,
    70, 221, 153, 101, 155, 167, 43, 172, 9, 129, 22, 39, 253, 19, 98, 108, 110, 79, 113, 224, 232,
    178, 185, 112, 104, 218, 246, 97, 228, 251, 34, 242, 193, 238, 210, 144, 12, 191, 179, 162,
    241, 81, 51, 145, 235, 249, 14, 239, 107, 49, 192, 214, 31, 181, 199, 106, 157, 184, 84, 204,
    176, 115, 121, 50, 45, 127, 4, 150, 254, 138, 236, 205, 93, 222, 114, 67, 29, 24, 72, 243, 141,
    128, 195, 78, 66, 215, 61, 156, 180,
];

fn perm(index: usize) -> usize {
    PERMUTATION[index & 255] as usize
}

fn gradient(hash: usize, x: f32, y: f32) -> f32 {
    let h = hash & 15;
    let u = if h < 8 { x } else { y };
    let v = if h < 4 {
        y
    } else if h == 12 || h == 14 {
        x
    } else {
        0.0
    };
    let u = if h & 1 != 0 { -u } else { u };
    let v = if h & 2 != 0 { -v } else { v };
    v + u
}

fn fade(t: f32) -> f32 {
    let t = if t > 1.0 { 1.0 } else { t };
    let square = t * t;
    let cube = square * t;
    let a = t * 6.0;
    let b = a - 15.0;
    let c = b * t;
    let d = c + 10.0;
    d * cube
}

// Preserve CVTTSS2SI's indefinite result outside its signed integer domain.
fn native_integer(value: f32) -> i32 {
    if value < 2147483648.0 {
        value as i32
    } else {
        i32::MIN
    }
}

/// Installed Unity 2D Perlin. Preserve separate f32 operations and the absolute input coordinates.
pub fn source_perlin(x: f32, y: f32) -> f32 {
    let x = x.abs();
    let y = y.abs();
    let ix = native_integer(x);
    let iy = native_integer(y);
    let x = x - ix as f32;
    let y = y - iy as f32;
    let px = perm(ix as usize);
    let px1 = perm((ix as usize).wrapping_add(1));
    let a = px + (iy as usize & 255);
    let b = px1 + (iy as usize & 255);
    let x1 = x - 1.0;
    let y1 = y - 1.0;
    let upper_left = gradient(perm(perm(a + 1)), x, y1);
    let upper_right = gradient(perm(perm(b + 1)), x1, y1);
    let lower_left = gradient(perm(perm(a)), x, y);
    let lower_right = gradient(perm(perm(b)), x1, y);
    let u = fade(x);
    let v = fade(y);
    let upper = (upper_right - upper_left) * u + upper_left;
    let lower = (lower_right - lower_left) * u + lower_left;
    let raw = (upper - lower) * v + lower;
    (raw + 0.69_f32) / 1.483_f32
}

#[derive(Clone, Copy)]
struct Octave {
    offset: [f32; 2],
    frequency: f32,
    amplitude: f32,
}

impl Octave {
    fn sample(self, xz: [f32; 2]) -> f32 {
        // Mono retains wider expression intermediates. Native call arguments spill to f32.
        let x = (self.offset[0] as f64 + xz[0] as f64 / self.frequency as f64) as f32;
        let y = (self.offset[1] as f64 + xz[1] as f64 / self.frequency as f64) as f32;
        (source_perlin(x, y) as f64 * self.amplitude as f64) as f32
    }
}

struct Noise {
    octaves: [Octave; 4],
    count: usize,
}

impl Noise {
    fn new(
        seed: i32,
        offset: [f32; 2],
        frequency: f32,
        amplitude: f32,
        count: usize,
        persistence: f32,
        lacunarity: f32,
    ) -> Self {
        let mut random = SourceRandom::new(seed);
        let mut frequency_scale = 1.0_f64;
        let mut amplitude_scale = 1.0_f64;
        let mut octaves = [Octave {
            offset: [0.0; 2],
            frequency: 1.0,
            amplitude: 0.0,
        }; 4];
        for octave in &mut octaves[..count] {
            // The source resets this stream for each Noise call, including color channels.
            let offset = offset.map(|base| {
                (random.next_range(-100000, 100000) as f64 * 1E-05_f32 as f64 + base as f64) as f32
            });
            *octave = Octave {
                offset,
                frequency: (frequency as f64 * frequency_scale) as f32,
                amplitude: (amplitude as f64 * amplitude_scale) as f32,
            };
            frequency_scale *= lacunarity as f64;
            amplitude_scale *= persistence as f64;
        }
        Self { octaves, count }
    }

    fn sample(&self, xz: [f32; 2]) -> f32 {
        let mut total = 0.0_f64;
        for octave in &self.octaves[..self.count] {
            total += octave.sample(xz) as f64;
        }
        total as f32
    }
}

struct Layer {
    noise: Noise,
    landscape: Octave,
    threshold: [f32; 2],
}

fn inverse_lerp(start: f32, end: f32, value: f32) -> f32 {
    if start == end {
        return 0.0;
    }
    ((value as f64 - start as f64) / (end as f64 - start as f64)).clamp(0.0, 1.0) as f32
}

fn random_offset(random: &mut SourceRandom) -> [f32; 2] {
    std::array::from_fn(|_| (random.next_range(-100000, 100000) as f64 * 1E-05_f32 as f64) as f32)
}

/// Source terrain inputs. The seed is the resolved terrain seed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerrainSettings {
    pub seed: i32,
    pub edge: f32,
    pub edge_gradient: f32,
}

/// Coordinates are source terrain X/Z, before presentation or physics rebasing.
pub struct TerrainEvaluator {
    settings: TerrainSettings,
    layers: [Layer; 4],
    colors: [Noise; 3],
}

impl TerrainEvaluator {
    pub fn new(settings: TerrainSettings) -> Self {
        let seed = settings.seed;
        let mut random = SourceRandom::new(seed);
        // TerrainGenerationJob constants, in source addition and seed draw order.
        let layers = [
            (1000.0, 100.0, 2, [0.85, 0.9]),
            (10000.0, 875.0, 3, [0.3, 0.5]),
            (3000.0, 250.0, 3, [0.45, 0.55]),
            (10000.0, 2500.0, 4, [0.5, 0.7]),
        ]
        .map(|(frequency, amplitude, count, threshold)| {
            let landscape = Octave {
                offset: random_offset(&mut random),
                frequency: 100000.0,
                amplitude: 1.0,
            };
            let offset = random_offset(&mut random);
            Layer {
                noise: Noise::new(seed, offset, frequency, amplitude, count, 0.05, 0.1),
                landscape,
                threshold,
            }
        });
        let mut random = SourceRandom::new(seed);
        let colors = [50000.0, 50000.0, 20000.0].map(|frequency| {
            Noise::new(
                seed,
                random_offset(&mut random),
                frequency,
                1.0,
                2,
                0.3,
                0.25,
            )
        });
        Self {
            settings,
            layers,
            colors,
        }
    }

    pub fn settings(&self) -> TerrainSettings {
        self.settings
    }

    pub fn base_height(&self, xz: [f32; 2], edge: bool) -> f32 {
        let mut height = 0.0_f64;
        for layer in &self.layers {
            let mask = inverse_lerp(
                layer.threshold[0],
                layer.threshold[1],
                layer.landscape.sample(xz),
            );
            height += layer.noise.sample(xz) as f64 * mask as f64;
        }
        if edge {
            let edge = self.settings.edge;
            let inner = (edge as f64 - self.settings.edge_gradient as f64) as f32;
            let x = inverse_lerp(edge, inner, xz[0].abs());
            let z = inverse_lerp(edge, inner, xz[1].abs());
            height *= x as f64 * z as f64;
        }
        height as f32
    }

    /// Raw source vertex channels. Biome overrides belong to the caller.
    pub fn color(&self, xz: [f32; 2], height: f32) -> [f32; 4] {
        [
            inverse_lerp(0.55, 0.65, self.colors[0].sample(xz)),
            inverse_lerp(0.3, 0.4, self.colors[1].sample(xz)),
            inverse_lerp(1600.0, 2500.0, height),
            inverse_lerp(0.6, 0.8, self.colors[2].sample(xz)),
        ]
    }
}
