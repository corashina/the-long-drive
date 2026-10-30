//! Integer, equal-edge triangle tessellation selected by the checked Grass hull program.
//! Concentric rings follow the D3D11 specification and Microsoft reference algorithm.
//! Primitive order is canonical here; source GPU leading-vertex order is device dependent.

#[derive(Default)]
pub struct GrassDomain {
    pub points: Vec<[f32; 3]>,
    pub triangles: Vec<[u16; 3]>,
}
impl GrassDomain {
    pub fn new(factor: u32) -> Result<Self, String> {
        if !matches!(factor, 2 | 4 | 8 | 16) {
            return Err(format!(
                "Unsupported source Grass tessellation factor {factor}"
            ));
        }
        let n = factor as usize;
        let point_count = 3 * (n / 2) * (n / 2 + 1) + 1;
        let triangle_count = 3 * n * n / 2;
        let mut result = Self {
            points: Vec::with_capacity(point_count),
            triangles: Vec::with_capacity(triangle_count),
        };
        let mut bases = [0usize; 9];
        for ring in 0..n / 2 {
            bases[ring] = result.points.len();
            let side = n - 2 * ring;
            // Powers of two have exact 16-bit reciprocal positions.
            let perpendicular = (((ring * 65536 / n) * 0xaaaa + 0x8000) >> 16) as i32;
            for edge in 0..3 {
                for step in 0..side {
                    let q = if edge == 1 {
                        ring + step
                    } else {
                        n - ring - step
                    };
                    let parallel = (q * 65536 / n) as i32 - (perpendicular + 1) / 2;
                    let (u, v) = match edge {
                        0 => (perpendicular, parallel),
                        1 => (parallel, perpendicular),
                        _ => (parallel, 65536 - parallel - perpendicular),
                    };
                    result.points.push([
                        u as f32 / 65536.0,
                        v as f32 / 65536.0,
                        (65536 - u - v) as f32 / 65536.0,
                    ]);
                }
            }
        }
        bases[n / 2] = result.points.len();
        result
            .points
            .push([21845.0 / 65536.0, 21845.0 / 65536.0, 21846.0 / 65536.0]);
        for ring in 1..=n / 2 {
            let inner_side = n - 2 * ring;
            let outer_side = inner_side + 2;
            for edge in 0..3 {
                let inside = |i: usize| -> u16 {
                    if inner_side == 0 {
                        bases[ring] as u16
                    } else {
                        (bases[ring] + (edge * inner_side + i) % (3 * inner_side)) as u16
                    }
                };
                let outside = |i: usize| -> u16 {
                    (bases[ring - 1] + (edge * outer_side + i) % (3 * outer_side)) as u16
                };
                let mut outer = 0;
                let mut inner = 0;
                result
                    .triangles
                    .push([outside(outer), outside(outer + 1), inside(inner)]);
                outer += 1;
                if ring == 1 {
                    // Equal transition factors advance both rows at every ruler split.
                    for _ in 0..n / 2 - 1 {
                        result
                            .triangles
                            .push([inside(inner), outside(outer), inside(inner + 1)]);
                        inner += 1;
                        result
                            .triangles
                            .push([outside(outer), outside(outer + 1), inside(inner)]);
                        outer += 1;
                    }
                    for _ in 0..n / 2 - 1 {
                        result
                            .triangles
                            .push([outside(outer), outside(outer + 1), inside(inner)]);
                        outer += 1;
                        result
                            .triangles
                            .push([inside(inner), outside(outer), inside(inner + 1)]);
                        inner += 1;
                    }
                } else {
                    for point in 0..inner_side {
                        if point < (inner_side + 1) / 2 {
                            result.triangles.push([
                                outside(outer),
                                inside(inner + 1),
                                inside(inner),
                            ]);
                            result.triangles.push([
                                outside(outer),
                                outside(outer + 1),
                                inside(inner + 1),
                            ]);
                        } else {
                            result.triangles.push([
                                inside(inner),
                                outside(outer),
                                outside(outer + 1),
                            ]);
                            result.triangles.push([
                                inside(inner),
                                outside(outer + 1),
                                inside(inner + 1),
                            ]);
                        }
                        inner += 1;
                        outer += 1;
                    }
                }
                result
                    .triangles
                    .push([outside(outer), outside(outer + 1), inside(inner)]);
            }
        }
        Ok(result)
    }
    pub fn retained_bytes(&self) -> usize {
        self.points.capacity() * std::mem::size_of::<[f32; 3]>()
            + self.triangles.capacity() * std::mem::size_of::<[u16; 3]>()
    }
}
