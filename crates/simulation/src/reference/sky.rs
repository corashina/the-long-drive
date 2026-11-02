//! Native one-pass sky geometry. Unity 2021.3.18f1 GetCubemapSkyboxVB.
//! These source-coordinate vertices are built once, before rendering.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct SkyVertex {
    pub position: [f32; 3],
    pub color: u32,
    pub direction: [f32; 3],
}
fn midpoint(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    let m: [f32; 3] = std::array::from_fn(|i| a[i] * 0.5 + b[i] * 0.5);
    let length = ((m[0] * m[0] + m[1] * m[1]) + m[2] * m[2]).sqrt();
    m.map(|v| v / length)
}
fn distance_squared(a: [f32; 3], b: [f32; 3]) -> f32 {
    let d: [f32; 3] = std::array::from_fn(|i| a[i] - b[i]);
    (d[0] * d[0] + d[1] * d[1]) + d[2] * d[2]
}
/// Preserves native scalar binary32 operations and triangle append order.
pub fn source_sky_vertices() -> Vec<SkyVertex> {
    let x = [1., 0., 0.];
    let y = [0., 1., 0.];
    let z = [0., 0., 1.];
    let nx = [-1., 0., 0.];
    let ny = [0., -1., 0.];
    let nz = [0., 0., -1.];
    let mut vertices = vec![
        y, nz, x, y, x, z, y, z, nx, y, nx, nz, ny, x, nz, ny, z, x, ny, nx, z, ny, nz, nx,
    ];
    for _ in 0..3 {
        let mut next = Vec::with_capacity(vertices.len() * 4);
        for triangle in vertices.chunks_exact(3) {
            let [a, b, c] = [triangle[0], triangle[1], triangle[2]];
            let ab = midpoint(a, b);
            let bc = midpoint(b, c);
            let ac = midpoint(a, c);
            next.extend_from_slice(&[a, ab, ac, ab, b, bc, bc, ac, ab, c, ac, bc]);
        }
        vertices = next;
    }
    for threshold in [0.5, 0.25] {
        let mut next = Vec::with_capacity(vertices.len() * 3);
        for triangle in vertices.chunks_exact(3) {
            let [a, b, c] = [triangle[0], triangle[1], triangle[2]];
            if a[1].abs().max(b[1].abs()).max(c[1].abs()) > threshold {
                next.extend_from_slice(triangle);
                continue;
            }
            let ba = (b[1] - a[1]).abs();
            let bc = (b[1] - c[1]).abs();
            let ca = (c[1] - a[1]).abs();
            let (d, e, f) = if bc > ba && ca > ba {
                (c, a, b)
            } else if ba > bc && ca > bc {
                (a, b, c)
            } else {
                (b, c, a)
            };
            let m = midpoint(d, e);
            let n = midpoint(d, f);
            next.extend_from_slice(&[d, m, n]);
            if distance_squared(n, e) > distance_squared(m, f) {
                next.extend_from_slice(&[m, e, f, n, m, f]);
            } else {
                next.extend_from_slice(&[n, m, e, n, e, f]);
            }
        }
        vertices = next;
    }
    vertices
        .into_iter()
        .map(|position| SkyVertex {
            position,
            color: u32::MAX,
            direction: position,
        })
        .collect()
}

/// Native D3D11 sky projection from the camera's implicit OpenGL-depth matrix.
/// Column-major. This includes the proven offscreen-target Y flip and reverse Z.
/// World-axis conversion and raster-target transport remain the caller's responsibility.
pub fn source_sky_gpu_projection(mut projection: [[f32; 4]; 4], near: f32) -> [[f32; 4]; 4] {
    let epsilon = 9.999999974752427e-7_f32;
    let sky_near = near * 0.009999999776482582_f32;
    projection[2][2] = epsilon - 1.;
    projection[2][3] = -1.;
    projection[3][2] = (epsilon - 2.) * sky_near;
    for column in &mut projection {
        column[1] = -column[1];
        column[2] = column[3] * 0.5 - column[2] * 0.5;
    }
    projection
}
