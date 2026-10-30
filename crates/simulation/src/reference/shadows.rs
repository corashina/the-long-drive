//! Recovered Unity StableFit operations. Inputs come from each live view and its caster bounds.
//! Conditional tests are equation fixtures; they are not source scene poses.

#[derive(Clone, Copy)]
pub struct ShadowFitInput {
    pub frustum_points: [[f32; 3]; 8],
    pub camera_basis: [[f32; 3]; 3],
    pub camera_position: [f32; 3],
    pub light_basis: [[f32; 3]; 3],
    pub caster_min: [f32; 3],
    pub caster_max: [f32; 3],
    pub near: f32,
    pub far: f32,
    pub vertical_fov: f32,
    pub shadow_distance: f32,
    pub split_fractions: [f32; 3],
    pub tile_size: [u32; 2],
    pub quality_near_offset: f32,
    pub shadow_strength: f32,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct ShadowCascade {
    pub sphere_center: [f32; 3],
    pub sphere_radius: f32,
    pub snapped_center: [f32; 3],
    pub light_position: [f32; 3],
    pub width_height: [f32; 2],
    pub texel_size: [f32; 2],
    pub near: f32,
    pub far: f32,
    pub min_view_distance: f32,
    pub max_view_distance: f32,
}
#[derive(Clone, Copy)]
pub struct ShadowFit {
    pub cascades: [ShadowCascade; 4],
    pub fade: [f32; 4],
    pub fade_center: [f32; 4],
}
fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|i| a[i] - b[i])
}
fn norm2(v: [f32; 3]) -> f32 {
    (v[0] * v[0] + v[1] * v[1]) + v[2] * v[2]
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[1] * b[1] + a[0] * b[0]) + a[2] * b[2]
}
fn affine(p: [f32; 3], basis: [[f32; 3]; 3], t: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|j| ((p[1] * basis[1][j] + p[0] * basis[0][j]) + p[2] * basis[2][j]) + t[j])
}

pub fn four_point_sphere(points: [[f32; 3]; 4]) -> Option<([f32; 3], f32)> {
    let mut matrix = [0.; 16];
    for column in 0..3 {
        for row in 0..4 {
            matrix[column * 4 + row] = points[row][column];
        }
    }
    matrix[12..].fill(1.);
    let d0 = determinant(matrix);
    if !d0.is_finite() || d0 == 0. {
        return None;
    }
    let squared: [f32; 4] = std::array::from_fn(|j| {
        let p = points[j];
        (p[1] * p[1] + p[0] * p[0]) + p[2] * p[2]
    });
    let center = std::array::from_fn(|column| {
        let mut replaced = matrix;
        replaced[column * 4..column * 4 + 4].copy_from_slice(&squared);
        ((f64::from(determinant(replaced)) * 0.5) / f64::from(d0)) as f32
    });
    let mut radius_matrix = [0.; 16];
    radius_matrix[..4].copy_from_slice(&squared);
    radius_matrix[4..].copy_from_slice(&matrix[..12]);
    let center_squared = (center[1] * center[1] + center[0] * center[0]) + center[2] * center[2];
    let radius = (center_squared - determinant(radius_matrix) / d0).sqrt();
    (radius.is_finite() && center.iter().all(|x| x.is_finite())).then_some((center, radius))
}
fn source_sphere(points: [[f32; 3]; 8]) -> Option<([f32; 3], f32)> {
    let p = [points[0], points[3], points[5], points[7]];
    for (a, b, c, d) in [(2, 3, 0, 1), (0, 1, 2, 3)] {
        let center = std::array::from_fn(|j| (p[a][j] + p[b][j]) * 0.5);
        let squared = norm2(sub(p[a], center));
        if norm2(sub(p[c], center)) <= squared && norm2(sub(p[d], center)) <= squared {
            return Some((center, squared.sqrt()));
        }
    }
    four_point_sphere(p)
}

/// Native SetupDirectionalLightShadowCamera 0x2b42a5..0x2b42c7.
/// A finite scale at or below epsilon is valid empty work, not malformed input.
pub fn frustum_scale(input: &ShadowFitInput) -> Result<f32, &'static str> {
    let scalar = [
        input.near,
        input.far,
        input.vertical_fov,
        input.shadow_distance,
        input.quality_near_offset,
        input.shadow_strength,
    ];
    let vectors = input
        .frustum_points
        .iter()
        .chain(input.camera_basis.iter())
        .chain(input.light_basis.iter())
        .chain([&input.camera_position, &input.caster_min, &input.caster_max]);
    if scalar.iter().any(|x| !x.is_finite())
        || vectors.flatten().any(|x| !x.is_finite())
        || input
            .split_fractions
            .iter()
            .any(|x| !x.is_finite() || *x <= 0. || *x >= 1.)
        || input.split_fractions.windows(2).any(|x| x[0] >= x[1])
        || !(0. ..=1.).contains(&input.shadow_strength)
        || input.near <= 0.
        || input.far <= input.near
        || input.shadow_distance <= 0.
        || input.vertical_fov <= 0.
        || input.vertical_fov >= 180.
        || input.tile_size.contains(&0)
        || input
            .caster_min
            .iter()
            .zip(input.caster_max)
            .any(|(a, b)| *a > b)
    {
        return Err("Invalid source perspective shadow inputs");
    }
    let k = 0.4000000059604645_f32 * (1. - input.vertical_fov / 180.).clamp(0., 1.);
    let q = norm2(input.frustum_points[6]).sqrt() / input.far;
    let q2 = q * q;
    let other = 1. - k;
    let a = ((q2 * other) * other - (q2 * k) * k) + k * k;
    let g = (a.sqrt() + k) / q2;
    let distance = input.shadow_distance.min(input.far);
    let scale = ((distance - input.near) * g) / (input.far - input.near);
    if !scale.is_finite() {
        return Err("Source shadow frustum scale is nonfinite");
    }
    Ok(scale)
}

pub fn fit_cascades(input: &ShadowFitInput) -> Result<ShadowFit, &'static str> {
    let scale = frustum_scale(input)?;
    if scale <= 9.999999747378752e-6 {
        return Err("Source shadow frustum has no valid portion");
    }
    let distance = input.shadow_distance.min(input.far);
    let k = 0.4000000059604645_f32 * (1. - input.vertical_fov / 180.).clamp(0., 1.);
    let box_center: [f32; 3] =
        std::array::from_fn(|j| (input.caster_min[j] + input.caster_max[j]) * 0.5);
    let diagonal = norm2(sub(input.caster_max, input.caster_min)).sqrt();
    let origin = std::array::from_fn(|j| {
        box_center[j] - (input.light_basis[2][j] * diagonal) * 1.2000000476837158_f32
    });
    let split = [
        0.,
        input.split_fractions[0],
        input.split_fractions[1],
        input.split_fractions[2],
        1.,
    ];
    let mut cascades = [ShadowCascade::default(); 4];
    for i in 0..4 {
        let a = scale * split[i];
        let b = scale * split[i + 1];
        let points = std::array::from_fn(|j| {
            let t = if j < 4 { a } else { b };
            let corner = j % 4;
            std::array::from_fn(|axis| {
                input.frustum_points[corner + 4][axis] * t
                    + input.frustum_points[corner][axis] * (1. - t)
            })
        });
        let (center, radius) = source_sphere(points).ok_or("Source shadow sphere is singular")?;
        let world_center = affine(center, input.camera_basis, input.camera_position);
        let local: [f32; 3] =
            std::array::from_fn(|j| dot(sub(world_center, origin), input.light_basis[j]));
        let lo: [f32; 3] = std::array::from_fn(|j| local[j] - radius);
        let hi: [f32; 3] = std::array::from_fn(|j| local[j] + radius);
        let extent = sub(hi, lo);
        let width_height: [f32; 2] =
            std::array::from_fn(|j| extent[j] + (extent[j] / input.tile_size[j] as f32) * 10.);
        let texel_size: [f32; 2] =
            std::array::from_fn(|j| width_height[j] / input.tile_size[j] as f32);
        let mid = std::array::from_fn(|j| (hi[j] + lo[j]) * 0.5);
        let unsnapped = affine(mid, input.light_basis, origin);
        // Native fmod uses binary64 and truncating division. rem_euclid/floor is not equivalent.
        let remainder: [f32; 2] = std::array::from_fn(|j| {
            let axis = input.light_basis[j];
            let value = (f64::from(unsnapped[1]) * f64::from(axis[1])
                + f64::from(unsnapped[0]) * f64::from(axis[0]))
                + f64::from(unsnapped[2]) * f64::from(axis[2]);
            (value % f64::from(texel_size[j])) as f32
        });
        let snapped_center = std::array::from_fn(|j| {
            (unsnapped[j] - input.light_basis[0][j] * remainder[0])
                - input.light_basis[1][j] * remainder[1]
        });
        let half_depth = extent[2] * 0.5;
        let light_position = std::array::from_fn(|j| {
            snapped_center[j] - (input.light_basis[2][j] * half_depth) * 1.2000000476837158_f32
        });
        cascades[i] = ShadowCascade {
            sphere_center: world_center,
            sphere_radius: radius,
            snapped_center,
            light_position,
            width_height,
            texel_size,
            near: half_depth * 0.10000000149011612_f32 - input.quality_near_offset.max(0.) * 10.,
            far: half_depth * 2.299999952316284_f32,
            min_view_distance: input.near + (input.far - input.near) * a,
            max_view_distance: distance.min(center[2].abs() + radius),
        };
    }
    for cascade in &cascades {
        let scalars = [
            cascade.sphere_radius,
            cascade.near,
            cascade.far,
            cascade.min_view_distance,
            cascade.max_view_distance,
        ];
        if cascade
            .sphere_center
            .iter()
            .chain(cascade.snapped_center.iter())
            .chain(cascade.light_position.iter())
            .chain(cascade.width_height.iter())
            .chain(cascade.texel_size.iter())
            .chain(scalars.iter())
            .any(|value| !value.is_finite())
            || cascade.sphere_radius <= 0.
            || cascade
                .width_height
                .iter()
                .chain(cascade.texel_size.iter())
                .any(|value| *value <= 0.)
            || cascade.near >= cascade.far
        {
            return Err("Source shadow fit is degenerate or nonfinite");
        }
    }
    let range = (1. - k) * distance;
    let start = range - distance * 0.20000000298023224_f32;
    let fade_scale = 1. / (range - start);
    let center = affine(
        [0., 0., -(k * distance)],
        input.camera_basis,
        input.camera_position,
    );
    if !fade_scale.is_finite() || center.iter().any(|value| !value.is_finite()) {
        return Err("Source shadow fade is nonfinite");
    }
    Ok(ShadowFit {
        cascades,
        fade: [
            1. - input.shadow_strength,
            input.far / distance,
            fade_scale,
            -start * fade_scale,
        ],
        fade_center: [center[0], center[1], center[2], 1.],
    })
}

// Matrix4x4f::GetDeterminant RVA0x476e20. Preserve binary64 scalar operation order.
fn determinant(input: [f32; 16]) -> f32 {
    let v1 = f64::from(input[6]);
    let v2 = f64::from(input[12]);
    let v3 = f64::from(input[0]);
    let v4 = f64::from(input[2]);
    let v5 = f64::from(input[8]);
    let v6 = f64::from(input[4]);
    let v7 = f64::from(input[9]);
    let v8 = f64::from(input[7]);
    let v9 = f64::from(input[10]);
    let v10 = f64::from(input[5]);
    let v11 = f64::from(input[1]);
    let v12 = f64::from(input[13]);
    let v13 = f64::from(input[11]);
    let v14 = f64::from(input[15]);
    let v15 = f64::from(input[14]);
    let v16 = v10 * v2;
    let v17 = v12 * v5;
    let v18 = v12 * v3;
    let v19 = v7 * v2;
    let v20 = v11 * v2;
    let v21 = v7 * v3;
    let v22 = f64::from(input[3]);
    let v23 = v10 * v3;
    let v24 = v19 * v1;
    let v25 = v10 * v5;
    let v26 = v17 * v1;
    let v27 = v11 * v5;
    let v28 = v26 * v22;
    let v29 = v11 * v6;
    let v30 = v24 * v22;
    let v31 = v30 - v28;
    let v32 = v12 * v6;
    let v33 = v7 * v6;
    let v34 = v16 * v9;
    let v35 = v32 * v9;
    let v36 = v34 * v22;
    let v37 = v35 * v22;
    let v38 = v31 - v36;
    let v39 = v25 * v15;
    let v40 = v38 + v37;
    let v41 = v33 * v15;
    let v42 = v39 * v22;
    let v43 = v41 * v22;
    let v44 = v40 + v42;
    let v45 = v17 * v4;
    let v46 = v18 * v9;
    let v47 = v44 - v43;
    let v48 = v16 * v4;
    let v49 = v45 * v8;
    let v50 = v20 * v9;
    let v51 = v46 * v8;
    let v52 = v48 * v13;
    let v53 = v50 * v8;
    let v54 = v19 * v4;
    let v55 = v32 * v4;
    let v56 = v25 * v4;
    let v57 = v54 * v8;
    let v58 = v55 * v13;
    let v59 = v33 * v4;
    let v60 = v47 - v57;
    let v61 = v20 * v1;
    let v62 = v18 * v1;
    let v63 = v60 + v49;
    let v64 = v61 * v13;
    let v65 = v62 * v13;
    let v66 = v63 + v53;
    let v67 = v27 * v15;
    let v68 = v27 * v1;
    let v69 = v66 - v51;
    let v70 = v67 * v8;
    let v71 = v21 * v15;
    let v72 = v21 * v1;
    let v73 = v69 - v70;
    let v74 = v71 * v8;
    let v75 = v73 + v74;
    let v76 = v75 + v52;
    let v77 = v29 * v9;
    let v78 = v29 * v15;
    let v79 = v76 - v58;
    let v80 = v78 * v13;
    let v81 = v79 - v64;
    let v82 = v23 * v9;
    let v83 = v23 * v15;
    let v84 = v81 + v65;
    let v85 = v83 * v13;
    let v86 = v84 + v80;
    let v87 = v56 * v14;
    let v88 = v59 * v14;
    let v89 = v86 - v85;
    let v90 = v68 * v14;
    let v91 = v72 * v14;
    let v92 = v77 * v14;
    let v93 = v89 - v87;
    let v94 = v93 + v88;
    let v95 = v94 + v90;
    let v96 = v95 - v91;
    let v97 = v96 - v92;
    let v98 = v82 * v14;
    let v99 = v97 + v98;
    let v100 = f64::from(v99 as f32);
    v100 as f32
}

/// Selected native quality2, automatic directional shadows, and >=480MB source hardware.
/// SetupShadowMapJobHeader uses binary32 3.8, truncation, next power of two, then shift1.
pub fn source_atlas_size(width: u32, height: u32) -> Option<u32> {
    if width == 0 || height == 0 || width.max(height) > 16384 {
        return None;
    }
    let requested = (3.8_f32 * width.max(height) as f32) as u32;
    Some((requested.checked_next_power_of_two()? >> 1).clamp(32, 4096))
}
