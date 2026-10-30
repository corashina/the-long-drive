//! Source cycle outputs retain Unity units.
use crate::reference::{CycleReference, GradientReference};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CyclePhase {
    Dawn,
    #[default]
    Day,
    Dusk,
    Night,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CloudOutput {
    pub color: [f32; 4],
    pub density: f32,
    pub speed: f32,
}

/// Clear-weather source signals. Angles use Unity Euler degrees; colors retain source channels.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CycleOutput {
    pub phase: CyclePhase,
    pub progress: f32,
    pub cycle_seconds: f64,
    pub directional_euler_degrees: [f32; 3],
    pub directional_color: [f32; 4],
    pub directional_intensity: f32,
    pub ambient: f32,
    pub reflection_intensity: f32,
    pub sky_exposure: f32,
    pub sky_tint: [f32; 4],
    pub ground_color: [f32; 4],
    pub atmosphere_thickness: f32,
    pub fog_density: f32,
    pub fog_color: [f32; 4],
    pub roadside_enabled: bool,
    pub biome_color: [f32; 4],
    /// Source temperature controls cloud density, needs, and engine heat.
    pub ambient_temperature: f32,
    /// Main-view ColorGrading temperature. None before the first terrain refresh keeps the profile value.
    pub grading_temperature: Option<f32>,
    /// Source order: low layer, high layer.
    pub clouds: [CloudOutput; 2],
    pub cloud_noise_coordinate: f32,
    pub cloud_noise: f32,
    pub star_color: [f32; 4],
    /// Add this local X rotation to each exported star pose.
    pub star_rotation_degrees: f32,
    pub flare_progress: f32,
    pub flare_brightness: f32,
    /// Active shader time. The cloud shader uses time / 20 with each layer's speed.
    pub shader_time_seconds: f64,
}

impl CycleOutput {
    /// Source night weight: 0 by day, 1 at night. Dawn and dusk follow the phase progress.
    pub fn night(&self) -> f32 {
        match self.phase {
            CyclePhase::Night => 1.,
            CyclePhase::Day => 0.,
            CyclePhase::Dawn => 1. - self.progress,
            CyclePhase::Dusk => self.progress,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CycleAdvance {
    pub simulation_delta: f32,
    pub cycle_extra: f32,
    pub cloud_scale: f32,
    pub sleep_progress: Option<f32>,
}

#[derive(Clone, Debug)]
pub struct CycleState {
    elapsed_seconds: f64,
    cycle_seconds: f64,
    cloud_coordinate: f32,
    star_rotation: f32,
    previous_cycle_extra: f32,
    output: CycleOutput,
    clear_output: CycleOutput,
    reference: CycleReference,
    sky_sample_time: f64,
    sampled_sky: [f32; 4],
    terrain_color: [f32; 4],
    next_biome_refresh: f64,
    /// Resolved source `DDS.fogModifier`.
    fog_modifier: f32,
}

impl CycleState {
    /// Initialize from a validated cycle reference.
    pub fn new(reference: &CycleReference, fog_modifier: f32) -> Self {
        let cycle_seconds = f64::from(reference.startup_offset_seconds);
        let output = evaluate(reference, fog_modifier, cycle_seconds, 0., 0., 0., [0.; 4]);
        Self {
            elapsed_seconds: 0.,
            cycle_seconds,
            cloud_coordinate: 0.,
            star_rotation: 0.,
            previous_cycle_extra: 0.,
            clear_output: output.clone(),
            sampled_sky: output.sky_tint,
            output,
            reference: reference.clone(),
            sky_sample_time: 0.,
            terrain_color: [0.; 4],
            next_biome_refresh: reference.biome_refresh_seconds as f64,
            fog_modifier,
        }
    }

    /// Advance one accepted simulation slice with its sampled sleep factors.
    pub fn advance(&mut self, reference: &CycleReference, advance: CycleAdvance) {
        let dt = advance.simulation_delta;
        let active = self.elapsed_seconds + f64::from(dt);
        let cycle = self.cycle_seconds + f64::from(dt) + f64::from(advance.cycle_extra);
        let cloud_step = dt * reference.cloud_perlin_speed * advance.cloud_scale;
        let star_step = reference.star_speed * dt + self.previous_cycle_extra;
        if dt < 0.
            || advance.cycle_extra < 0.
            || advance.cloud_scale < 0.
            || !dt.is_finite()
            || !advance.cycle_extra.is_finite()
            || !advance.cloud_scale.is_finite()
            || !active.is_finite()
            || !cycle.is_finite()
            || !cloud_step.is_finite()
            || !star_step.is_finite()
            || advance.sleep_progress.is_some_and(|p| !p.is_finite())
        {
            return;
        }
        // Source rotates stars before replacing tekerve for this fixed update.
        self.star_rotation = (self.star_rotation + star_step) % 360.;
        self.previous_cycle_extra = advance.cycle_extra;
        self.cloud_coordinate += cloud_step;
        if self.cloud_coordinate > 0. {
            self.cloud_coordinate -= self.cloud_coordinate.ceil();
        }
        self.elapsed_seconds = active;
        let grading_temperature = self.output.grading_temperature;
        self.output = evaluate(
            reference,
            self.fog_modifier,
            cycle,
            active,
            self.cloud_coordinate,
            self.star_rotation,
            self.terrain_color,
        );
        self.output.grading_temperature = grading_temperature;
        self.set_sleep_progress(reference, advance.sleep_progress);
        self.clear_output = self.output.clone();
        self.cycle_seconds = if cycle as f32 > reference.day_seconds + reference.night_seconds {
            0.
        } else {
            cycle
        };
    }

    /// Sandbox: jumps to `seconds` of the source cycle. The next advance evaluates it.
    pub fn set_cycle_seconds(&mut self, seconds: f64) {
        self.cycle_seconds = seconds;
    }

    pub fn capture_save(&self) -> crate::save::CycleSnapshot {
        crate::save::CycleSnapshot {
            elapsed_seconds: self.elapsed_seconds,
            cycle_seconds: self.cycle_seconds,
            evaluated_cycle_seconds: self.output.cycle_seconds,
            cloud_coordinate: self.cloud_coordinate,
            star_rotation: self.star_rotation,
        }
    }
    /// Restore validated scalar state without advancing the source cycle.
    pub fn restore_save(&mut self, reference: &CycleReference, saved: &crate::save::CycleSnapshot) {
        self.elapsed_seconds = saved.elapsed_seconds;
        self.cycle_seconds = saved.cycle_seconds;
        self.cloud_coordinate = saved.cloud_coordinate;
        self.star_rotation = saved.star_rotation;
        let grading_temperature = self.output.grading_temperature;
        self.output = evaluate(
            reference,
            self.fog_modifier,
            saved.evaluated_cycle_seconds,
            saved.elapsed_seconds,
            saved.cloud_coordinate,
            saved.star_rotation,
            self.terrain_color,
        );
        self.output.grading_temperature = grading_temperature;
        self.clear_output = self.output.clone();
        self.sampled_sky = self.output.sky_tint;
        self.sky_sample_time = self.elapsed_seconds;
    }
    /// Felho changes material speed at sleep endpoints without advancing time.
    pub fn set_sleep_progress(&mut self, r: &CycleReference, progress: Option<f32>) {
        let p = progress.unwrap_or(1.);
        for (index, (cloud, source)) in self.output.clouds.iter_mut().zip(&r.clouds).enumerate() {
            cloud.speed = if p < 0.5 {
                lerp(source.mid_speed, source.max_speed, inverse_lerp(0., 0.5, p))
            } else {
                lerp(source.min_speed, source.mid_speed, inverse_lerp(0.5, 1., p))
            };
            self.clear_output.clouds[index].speed = cloud.speed;
        }
    }

    /// Compose from retained clear values. SetBiome samples sky tint once per source second.
    pub fn compose_weather(
        &mut self,
        weather: &crate::reference::WeatherReference,
        influence: crate::weather::StormInfluence,
    ) {
        self.output = self.clear_output.clone();
        let w = influence.strength.clamp(0., 1.);
        let r = &self.reference;
        let s = &weather.storm;
        let o = &mut self.output;
        if self.elapsed_seconds >= self.sky_sample_time + f64::from(r.biome_refresh_seconds) {
            self.sky_sample_time = self.elapsed_seconds;
            self.sampled_sky = color_lerp(self.clear_output.sky_tint, s.sky_color, w);
        }
        if influence.id.is_none() || w == 0. {
            return;
        }
        o.sky_tint = self.sampled_sky;
        o.directional_intensity *= lerp(1., s.intensity_multipler, w);
        o.ambient *= lerp(1., s.ambient_multipler, w);
        let p = o.progress;
        let biome = o.biome_color;
        let cube = w * w * w;
        let day_ground = color_lerp(
            biome_color(&r.day_ground_colors, biome),
            s.sky_ground_color,
            w,
        );
        let night_ground = biome_color(&r.night_ground_colors, biome);
        o.ground_color = match o.phase {
            CyclePhase::Day => day_ground,
            CyclePhase::Night => night_ground,
            CyclePhase::Dawn => color_lerp(night_ground, day_ground, p),
            CyclePhase::Dusk => color_lerp(day_ground, night_ground, p),
        };
        o.atmosphere_thickness = match o.phase {
            CyclePhase::Night => 0.,
            CyclePhase::Day => lerp(1., s.atmosphere_thickness, cube),
            _ => lerp(
                s.atmosphere_thickness,
                self.clear_output.atmosphere_thickness,
                (1. - cube).max(p),
            ),
        };
        let day_density = lerp(
            biome_float(&r.day_fog_densities, biome),
            s.fog_density,
            cube,
        );
        let night_density = biome_float(&r.night_fog_densities, biome);
        let day_fog = color_lerp(biome_color(&r.day_fog_colors, biome), s.fog_color, w);
        let night_fog = biome_color(&r.night_fog_colors, biome);
        let (density, color) = match o.phase {
            CyclePhase::Day => (day_density, day_fog),
            CyclePhase::Night => (night_density, night_fog),
            CyclePhase::Dawn => (
                lerp(night_density, day_density, inverse_lerp(0., 0.5, p)),
                color_lerp(night_fog, day_fog, inverse_lerp(0.5, 1., p)),
            ),
            CyclePhase::Dusk => (
                lerp(day_density, night_density, inverse_lerp(0.5, 1., p)),
                color_lerp(day_fog, night_fog, inverse_lerp(0., 0.5, p)),
            ),
        };
        o.fog_density = density * self.fog_modifier;
        o.fog_color = color;
        for (cloud, source) in o.clouds.iter_mut().zip(&r.clouds) {
            let day = color_lerp(source.day_color, s.cloud_color, w);
            let transition = color_lerp(source.transition_color, s.cloud_color, w);
            cloud.color = match o.phase {
                CyclePhase::Day => day,
                CyclePhase::Night => source.night_color,
                CyclePhase::Dawn if p < 0.5 => {
                    color_lerp(source.night_color, transition, inverse_lerp(0.3, 0.5, p))
                }
                CyclePhase::Dawn => color_lerp(transition, day, (p - 0.5) * 2.),
                CyclePhase::Dusk if p < 0.5 => color_lerp(day, transition, p * 2.),
                CyclePhase::Dusk => {
                    color_lerp(transition, source.night_color, inverse_lerp(0.5, 0.7, p))
                }
            };
        }
        let day_flare = biome_float(&r.flare_day, biome) * (1. - w);
        let night_flare = biome_float(&r.flare_night, biome);
        o.flare_brightness = match o.phase {
            CyclePhase::Day => day_flare,
            CyclePhase::Night => night_flare,
            CyclePhase::Dawn => lerp3(
                night_flare,
                biome_float(&r.flare_dawn, biome) * (1. - w),
                day_flare,
                o.flare_progress,
            ),
            CyclePhase::Dusk => lerp3(
                day_flare,
                biome_float(&r.flare_dusk, biome) * (1. - w),
                night_flare,
                o.flare_progress,
            ),
        };
    }

    pub fn needs_terrain_color(&self, dt: f64) -> bool {
        self.elapsed_seconds + dt >= self.next_biome_refresh
    }
    pub fn set_terrain_color(&mut self, reference: &CycleReference, color: &[f32; 4], dt: f64) {
        if self.needs_terrain_color(dt) {
            self.terrain_color = *color;
            self.output.grading_temperature =
                Some(biome_float(&reference.temperature_values, *color));
            self.next_biome_refresh =
                self.elapsed_seconds + dt + reference.biome_refresh_seconds as f64;
        }
    }
    pub fn reset(&mut self, reference: &CycleReference) {
        *self = Self::new(reference, self.fog_modifier);
    }

    /// Return monotonic active play time, without the source startup offset.
    pub fn elapsed_seconds(&self) -> f64 {
        self.elapsed_seconds
    }

    pub fn output(&self) -> &CycleOutput {
        &self.output
    }
}

fn evaluate(
    r: &CycleReference,
    fog_modifier: f32,
    seconds: f64,
    active: f64,
    cloud_x: f32,
    star_rotation: f32,
    biome: [f32; 4],
) -> CycleOutput {
    let t = seconds as f32;
    let (phase, p) = if t > r.day_seconds {
        (
            CyclePhase::Night,
            inverse_lerp(r.day_seconds, r.day_seconds + r.night_seconds, t),
        )
    } else if t > r.day_seconds - r.dusk_seconds {
        (
            CyclePhase::Dusk,
            inverse_lerp(r.day_seconds - r.dusk_seconds, r.day_seconds, t),
        )
    } else if t > r.dawn_seconds {
        (
            CyclePhase::Day,
            inverse_lerp(r.dawn_seconds, r.day_seconds - r.dusk_seconds, t),
        )
    } else {
        (CyclePhase::Dawn, inverse_lerp(0., r.dawn_seconds, t))
    };
    let (pitch, yaw, intensity, ambient, exposure, gradient_time) = match phase {
        CyclePhase::Dawn => (
            lerp(r.midnight_angle_dawn, r.day_angle_min, p),
            lerp(0., r.day_yaw_degrees, p),
            lerp(0., r.day_intensity, p),
            lerp(r.night_ambient, r.day_ambient, p),
            lerp(r.min_exposure, r.max_exposure, p),
            1. - p,
        ),
        CyclePhase::Day => (
            lerp(r.day_angle_min, r.day_angle_max, p),
            r.day_yaw_degrees,
            lerp(
                r.noon_intensity,
                r.day_intensity,
                inverse_lerp(0.45, 0.35, p).max(inverse_lerp(0.55, 0.7, p)),
            ),
            r.day_ambient,
            r.max_exposure,
            0.,
        ),
        CyclePhase::Dusk => (
            lerp(r.day_angle_max, r.midnight_angle_dusk, p),
            lerp(r.day_yaw_degrees, 0., p),
            lerp(r.day_intensity, 0., p),
            lerp(r.day_ambient, r.night_ambient, p),
            lerp(r.max_exposure, r.min_exposure, p),
            p,
        ),
        CyclePhase::Night => (
            lerp(r.min_moon_angle, r.max_moon_angle, p),
            0.,
            r.night_intensity,
            r.night_ambient,
            r.max_exposure,
            1.,
        ),
    };
    let day_temp = biome_float(&r.temp_days, biome);
    let night_temp = biome_float(&r.temp_nights, biome);
    let noon_temp = biome_float(&r.temp_add_noons, biome);
    let temp = match phase {
        CyclePhase::Dawn => lerp(night_temp, day_temp, p),
        CyclePhase::Day => {
            day_temp
                + lerp(0., noon_temp, inverse_lerp(0.3, 0.55, p))
                + (lerp(noon_temp, 0., inverse_lerp(0.7, 9., p)) - noon_temp)
        }
        CyclePhase::Dusk => lerp(day_temp, night_temp, p),
        CyclePhase::Night => night_temp,
    };
    let fog_density_day = biome_float(&r.day_fog_densities, biome);
    let fog_density_night = biome_float(&r.night_fog_densities, biome);
    let fog_day = biome_color(&r.day_fog_colors, biome);
    let fog_night = biome_color(&r.night_fog_colors, biome);
    let (fog_density, fog_color) = match phase {
        CyclePhase::Dawn => (
            lerp(fog_density_night, fog_density_day, inverse_lerp(0., 0.5, p)),
            color_lerp(fog_night, fog_day, inverse_lerp(0.5, 1., p)),
        ),
        CyclePhase::Day => (fog_density_day, fog_day),
        CyclePhase::Dusk => (
            lerp(fog_density_day, fog_density_night, inverse_lerp(0.5, 1., p)),
            color_lerp(fog_day, fog_night, inverse_lerp(0., 0.5, p)),
        ),
        CyclePhase::Night => (fog_density_night, fog_night),
    };
    let cloud_noise = (native_cloud_noise(cloud_x) - 0.5) * 2. * r.cloud_perlin_spread;
    let cloud_density = inverse_lerp(r.temp_days[0] + r.temp_add_noons[0], r.temp_nights[0], temp);
    let clouds = std::array::from_fn(|i| {
        let cloud = &r.clouds[i];
        let color = match phase {
            CyclePhase::Dawn if p < 0.5 => color_lerp(
                cloud.night_color,
                cloud.transition_color,
                inverse_lerp(0.3, 0.5, p),
            ),
            CyclePhase::Dawn => color_lerp(cloud.transition_color, cloud.day_color, (p - 0.5) * 2.),
            CyclePhase::Day => cloud.day_color,
            CyclePhase::Dusk if p < 0.5 => {
                color_lerp(cloud.day_color, cloud.transition_color, p * 2.)
            }
            CyclePhase::Dusk => color_lerp(
                cloud.transition_color,
                cloud.night_color,
                inverse_lerp(0.5, 0.7, p),
            ),
            CyclePhase::Night => cloud.night_color,
        };
        CloudOutput {
            color,
            density: lerp(
                cloud.min_density,
                cloud.max_density,
                cloud_density + cloud_noise,
            ),
            // Start calls Felho(1). FixedUpdate does not change _Speed.
            speed: lerp(cloud.min_speed, cloud.mid_speed, inverse_lerp(0.5, 1., 1.)),
        }
    });
    let flare_night = biome_float(&r.flare_night, biome);
    let flare_day = biome_float(&r.flare_day, biome);
    let mut flare_progress = p;
    let flare_brightness = match phase {
        CyclePhase::Dawn => {
            if flare_progress < r.flare_lerp_start {
                flare_progress = lerp(
                    0.,
                    r.flare_lerp_end,
                    inverse_lerp(r.flare_lerp_start, r.flare_lerp_end, flare_progress),
                );
            }
            lerp3(
                flare_night,
                biome_float(&r.flare_dawn, biome),
                flare_day,
                flare_progress,
            )
        }
        CyclePhase::Day => flare_day,
        CyclePhase::Dusk => {
            if flare_progress > r.flare_lerp_start {
                flare_progress = lerp(
                    r.flare_lerp_start,
                    1.,
                    inverse_lerp(r.flare_lerp_start, r.flare_lerp_end, flare_progress),
                );
            }
            lerp3(
                flare_day,
                biome_float(&r.flare_dusk, biome),
                flare_night,
                flare_progress,
            )
        }
        CyclePhase::Night => flare_night,
    };
    CycleOutput {
        phase,
        progress: p,
        cycle_seconds: seconds,
        directional_euler_degrees: [pitch, yaw, 0.],
        directional_color: gradient(&r.color, gradient_time),
        directional_intensity: intensity,
        ambient,
        reflection_intensity: lerp(
            r.min_reflection,
            r.max_reflection,
            inverse_lerp(0., r.day_ambient, ambient),
        ),
        sky_exposure: exposure,
        sky_tint: biome_color(&r.sky_colors, biome),
        ground_color: match phase {
            CyclePhase::Dawn => color_lerp(
                biome_color(&r.night_ground_colors, biome),
                biome_color(&r.day_ground_colors, biome),
                p,
            ),
            CyclePhase::Day => biome_color(&r.day_ground_colors, biome),
            CyclePhase::Dusk => color_lerp(
                biome_color(&r.day_ground_colors, biome),
                biome_color(&r.night_ground_colors, biome),
                p,
            ),
            CyclePhase::Night => biome_color(&r.night_ground_colors, biome),
        },
        atmosphere_thickness: match phase {
            CyclePhase::Night => 0.,
            CyclePhase::Day => 1.,
            _ => gradient(&r.atmosphere_thickness, gradient_time)[0] * 255.,
        },
        fog_density: fog_density * fog_modifier,
        fog_color,
        roadside_enabled: match phase {
            CyclePhase::Dawn => p < 0.5,
            CyclePhase::Day => false,
            CyclePhase::Dusk => p > 0.5,
            CyclePhase::Night => true,
        },
        biome_color: biome,
        ambient_temperature: temp,
        grading_temperature: None,
        clouds,
        cloud_noise_coordinate: cloud_x,
        cloud_noise,
        star_color: match phase {
            CyclePhase::Dawn => color_lerp(r.star_color_night, r.star_color_day, p),
            CyclePhase::Day => r.star_color_day,
            CyclePhase::Dusk => color_lerp(r.star_color_day, r.star_color_night, p),
            CyclePhase::Night => r.star_color_night,
        },
        star_rotation_degrees: star_rotation,
        flare_progress,
        flare_brightness,
        shader_time_seconds: active,
    }
}

pub(crate) fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0., 1.)
}
pub(crate) fn inverse_lerp(a: f32, b: f32, value: f32) -> f32 {
    if a == b {
        0.
    } else {
        ((value - a) / (b - a)).clamp(0., 1.)
    }
}
fn color_lerp(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    std::array::from_fn(|i| lerp(a[i], b[i], t))
}
fn biome_color(colors: &[[f32; 4]; 3], biome: [f32; 4]) -> [f32; 4] {
    color_lerp(
        color_lerp(colors[2], colors[0], biome[0]),
        colors[1],
        biome[2],
    )
}
fn biome_float(values: &[f32; 3], biome: [f32; 4]) -> f32 {
    lerp(lerp(values[2], values[0], biome[0]), values[1], biome[2])
}
fn lerp3(a: f32, b: f32, c: f32, t: f32) -> f32 {
    if t < 0.5 {
        lerp(a, b, inverse_lerp(0., 0.5, t))
    } else {
        lerp(b, c, inverse_lerp(0.5, 1., t))
    }
}
pub fn gradient(g: &GradientReference, time: f32) -> [f32; 4] {
    let time = time.clamp(0., 1.);
    let mut color = g.colors[0].value;
    for keys in g.colors.windows(2) {
        if time < keys[1].time {
            color = if g.mode == 1 {
                keys[0].value
            } else {
                color_lerp(
                    keys[0].value,
                    keys[1].value,
                    inverse_lerp(keys[0].time, keys[1].time, time),
                )
            };
            break;
        }
        color = keys[1].value;
    }
    let mut alpha = g.alphas[0].value;
    for keys in g.alphas.windows(2) {
        if time < keys[1].time {
            alpha = if g.mode == 1 {
                keys[0].value
            } else {
                lerp(
                    keys[0].value,
                    keys[1].value,
                    inverse_lerp(keys[0].time, keys[1].time, time),
                )
            };
            break;
        }
        alpha = keys[1].value;
    }
    color[3] = alpha;
    color
}

/// Installed UnityPlayer's checked slice: x in [-1, 0], y = 0. Keep each f32 operation separate.
fn native_cloud_noise(x: f32) -> f32 {
    let t = x.abs();
    if t == 1. {
        return 0.69_f32 / 1.483_f32;
    }
    let t2 = t * t;
    let t3 = t2 * t;
    let a = t * 6.;
    let b = a - 15.;
    let c = b * t;
    let d = c + 10.;
    let fade = d * t3;
    let g1 = t - 1.;
    let delta = g1 - t;
    let raw = delta * fade;
    let raw = raw + t;
    let value = raw + 0.69_f32;
    value / 1.483_f32
}

#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LightOutput {
    pub id: crate::reference::LightId,
    pub enabled: bool,
    pub intensity: f32,
}
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LampMaterialOutput {
    pub renderer: crate::reference::NodeId,
    pub asset: crate::reference::AssetId,
    pub material_index: u32,
}
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ControlPoseOutput {
    pub node: crate::reference::NodeId,
    pub local_pose: crate::reference::Transform,
}
#[derive(Clone, Debug)]
pub struct HeadlightState {
    pub current_state: usize,
    pub requested_state: usize,
    pub energy: f32,
    pub enabled: bool,
    pub on: bool,
    refreshed_state: usize,
    energy_move: f32,
    state_count: usize,
}
impl HeadlightState {
    pub fn new(reference: &crate::reference::LightControllerReference) -> Self {
        Self {
            current_state: reference.current_state,
            requested_state: reference.requested_state,
            energy: reference.energy,
            enabled: reference.enabled,
            on: reference.startup_on,
            refreshed_state: reference.current_state,
            energy_move: reference.energy_move,
            state_count: reference.states.len(),
        }
    }
    /// Startup state of a world car `headlightscript` or `rearlights` controller.
    pub fn world(reference: &crate::reference::WorldLightReference) -> Self {
        Self {
            current_state: reference.current_state,
            requested_state: reference.requested_state,
            energy: reference.energy,
            enabled: reference.enabled,
            on: reference.startup_on,
            refreshed_state: reference.current_state,
            energy_move: reference.energy_move,
            state_count: reference.states.len(),
        }
    }
    /// Source `headlightscript` with its part removed: dark, and lit again from state 0.
    pub fn unmount(&mut self) {
        self.current_state = 0;
        self.refreshed_state = 0;
        self.energy = 0.;
        self.enabled = true;
    }
    /// The state that the last refresh shows.
    pub fn refreshed_state(&self) -> usize {
        self.refreshed_state
    }
    pub fn capture_save(&self, source: crate::save::SourceKey) -> crate::save::ControllerSnapshot {
        crate::save::ControllerSnapshot {
            source,
            current_state: self.current_state,
            requested_state: self.requested_state,
            refreshed_state: self.refreshed_state,
            energy: self.energy,
            enabled: self.enabled,
            on: self.on,
            switch_motion: None,
            switch_poses: Vec::new(),
        }
    }
    fn restore_save(&mut self, saved: &crate::save::ControllerSnapshot) {
        self.current_state = saved.current_state;
        self.requested_state = saved.requested_state;
        self.refreshed_state = saved.refreshed_state;
        self.energy = saved.energy;
        self.enabled = saved.enabled;
        self.on = saved.on;
    }
    pub fn request(&mut self, state: u8) {
        let state = usize::from(state);
        if state < self.state_count && state != self.requested_state && state != self.current_state
        {
            self.requested_state = state;
            self.enabled = true;
        }
    }
    pub fn update(&mut self, dt: f32) {
        if !self.enabled || !dt.is_finite() || dt < 0. {
            return;
        }
        if self.requested_state == self.current_state {
            if self.energy < 1. {
                self.energy = (self.energy + dt * self.energy_move).clamp(0., 1.);
            } else {
                self.enabled = false;
            }
            self.refreshed_state = self.current_state;
        } else if self.energy > 0. {
            self.energy = (self.energy - dt * self.energy_move).clamp(0., 1.);
            self.refreshed_state = self.current_state;
        } else {
            // Source RefreshLights runs before the current-state assignment.
            self.refreshed_state = self.current_state;
            self.current_state = self.requested_state;
        }
    }
    pub fn current_state(&self) -> u8 {
        self.current_state as u8
    }
    pub fn energy(&self) -> f32 {
        self.energy
    }
}
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct NodeVisibilityOutput {
    pub node: crate::reference::NodeId,
    pub visible: bool,
}
#[derive(Clone, Copy)]
enum SwitchMotion {
    Initial,
    Moving(f32),
}
pub struct LightingState {
    pub cycle: CycleState,
    pub controllers: Vec<HeadlightState>,
    pub lights: Vec<LightOutput>,
    pub materials: Vec<LampMaterialOutput>,
    pub poses: Vec<ControlPoseOutput>,
    visibility: Vec<NodeVisibilityOutput>,
    pose_progress: Vec<Option<SwitchMotion>>,
}
impl LightingState {
    pub fn new(
        reference: &crate::reference::LightingRecord,
        distances: &crate::world::DrawDistances,
    ) -> Self {
        let mut state = Self {
            cycle: CycleState::new(&reference.cycle, distances.fog_modifier),
            visibility: reference
                .controllers
                .iter()
                .flat_map(|c| &c.states)
                .flat_map(|s| {
                    s.working_nodes
                        .iter()
                        .map(|node| NodeVisibilityOutput {
                            node: *node,
                            visible: !s.broken,
                        })
                        .chain(s.broken_nodes.iter().map(|node| NodeVisibilityOutput {
                            node: *node,
                            visible: s.broken,
                        }))
                })
                .collect(),
            pose_progress: vec![None; reference.controllers.len()],
            controllers: reference
                .controllers
                .iter()
                .map(HeadlightState::new)
                .collect(),
            lights: reference
                .lights
                .iter()
                .map(|l| LightOutput {
                    id: l.id,
                    enabled: l.enabled,
                    intensity: l.startup_intensity,
                })
                .collect(),
            materials: Vec::with_capacity(
                reference
                    .controllers
                    .iter()
                    .flat_map(|c| &c.states)
                    .map(|s| s.renderers.len())
                    .sum::<usize>(),
            ),
            poses: Vec::with_capacity(
                reference
                    .controllers
                    .iter()
                    .map(|c| c.animated_poses.len())
                    .sum(),
            ),
        };
        state.reset(reference);
        state
    }
    pub fn capture_save(
        &self,
        reference: &crate::reference::LightingRecord,
        ids: &crate::save::IdentityMap,
    ) -> Result<crate::save::LightingSnapshot, crate::save::SaveError> {
        use crate::save::{LocalPose, SwitchMotionSave, SwitchPoseSnapshot, identity::source_key};
        let mut controllers = Vec::with_capacity(self.controllers.len());
        for (index, c) in reference.controllers.iter().enumerate() {
            let mut saved =
                self.controllers[index].capture_save(source_key(&ids.controllers, c.id)?);
            saved.switch_motion = self.pose_progress[index].map(|m| match m {
                SwitchMotion::Initial => SwitchMotionSave::Initial,
                SwitchMotion::Moving(progress) => SwitchMotionSave::Moving { progress },
            });
            for pose in &c.animated_poses {
                let current = self
                    .poses
                    .iter()
                    .find(|p| p.node == pose.node)
                    .ok_or_else(|| {
                        crate::save::SaveError::InvalidLinks("Missing switch pose".into())
                    })?;
                saved.switch_poses.push(SwitchPoseSnapshot {
                    node: source_key(&ids.nodes, pose.node)?,
                    pose: LocalPose {
                        position: current.local_pose.translation,
                        rotation: current.local_pose.rotation,
                    },
                });
            }
            saved.switch_poses.sort_by(|a, b| a.node.cmp(&b.node));
            controllers.push(saved);
        }
        controllers.sort_by(|a, b| a.source.cmp(&b.source));
        Ok(crate::save::LightingSnapshot {
            cycle: self.cycle.capture_save(),
            controllers,
        })
    }
    /// Restore validated state.
    pub fn restore_save(
        &mut self,
        reference: &crate::reference::LightingRecord,
        ids: &crate::save::IdentityMap,
        saved: &crate::save::LightingSnapshot,
    ) -> Result<(), crate::save::SaveError> {
        use crate::save::{SaveError, SwitchMotionSave};
        if saved.controllers.len() != self.controllers.len() {
            return Err(SaveError::InvalidLinks(
                "Missing lamp controller state".into(),
            ));
        }
        self.cycle.restore_save(&reference.cycle, &saved.cycle);
        for record in &saved.controllers {
            let id = ids
                .controllers
                .get(&record.source)
                .ok_or_else(|| SaveError::InvalidLinks("Unknown lamp controller".into()))?;
            let index = reference
                .controllers
                .iter()
                .position(|c| c.id == *id)
                .ok_or_else(|| SaveError::InvalidLinks("Missing lamp controller".into()))?;
            self.controllers[index].restore_save(record);
            self.pose_progress[index] = record.switch_motion.map(|m| match m {
                SwitchMotionSave::Initial => SwitchMotion::Initial,
                SwitchMotionSave::Moving { progress } => SwitchMotion::Moving(progress),
            });
            for pose in &record.switch_poses {
                let node = ids
                    .nodes
                    .get(&pose.node)
                    .ok_or_else(|| SaveError::InvalidLinks("Unknown switch node".into()))?;
                let current = self
                    .poses
                    .iter_mut()
                    .find(|p| p.node == *node)
                    .ok_or_else(|| SaveError::InvalidLinks("Missing switch pose".into()))?;
                current.local_pose.translation = pose.pose.position;
                current.local_pose.rotation = pose.pose.rotation;
            }
        }
        for index in 0..reference.controllers.len() {
            if reference.controllers[index].kind == crate::reference::LightControllerKind::Switch {
                self.refresh_switch(reference, index);
            }
        }
        Ok(())
    }

    pub fn reset(&mut self, reference: &crate::reference::LightingRecord) {
        self.cycle.reset(&reference.cycle);
        self.materials.clear();
        self.poses.clear();
        self.pose_progress.fill(None);
        for (out, light) in self.lights.iter_mut().zip(&reference.lights) {
            *out = LightOutput {
                id: light.id,
                enabled: light.enabled,
                intensity: light.startup_intensity,
            };
        }
        for (state, control) in self.controllers.iter_mut().zip(&reference.controllers) {
            *state = HeadlightState::new(control);
            for lamp in &control.states {
                if let Some(material) = if control.startup_on {
                    &lamp.on_material
                } else {
                    &lamp.off_material
                } {
                    for &renderer in &lamp.renderers {
                        if let Some(out) = self
                            .materials
                            .iter_mut()
                            .find(|out| out.renderer == renderer)
                        {
                            *out = LampMaterialOutput {
                                renderer,
                                asset: material.asset,
                                material_index: material.gltf_material,
                            };
                        } else {
                            self.materials.push(LampMaterialOutput {
                                renderer,
                                asset: material.asset,
                                material_index: material.gltf_material,
                            });
                        }
                    }
                }
            }
            for pose in &control.animated_poses {
                if let Some(local_pose) = pose
                    .states
                    .get(usize::from(control.startup_on ^ control.reverse_switch))
                {
                    self.poses.push(ControlPoseOutput {
                        node: pose.node,
                        local_pose: crate::reference::Transform {
                            translation: local_pose.translation,
                            rotation: local_pose.rotation,
                            scale: [1.; 3],
                        },
                    });
                }
            }
        }
        for index in 0..reference.controllers.len() {
            if reference.controllers[index].kind == crate::reference::LightControllerKind::Switch {
                self.refresh_switch(reference, index);
            }
        }
    }
    pub fn use_control(
        &mut self,
        reference: &crate::reference::LightingRecord,
        control: crate::reference::LightControlId,
    ) {
        let Some(index) = reference.controllers.iter().position(|c| c.id == control) else {
            return;
        };
        if reference.controllers[index].kind != crate::reference::LightControllerKind::Switch {
            return;
        }
        self.controllers[index].on = !self.controllers[index].on;
        if !reference.controllers[index].animated_poses.is_empty()
            && self.pose_progress[index].is_none()
        {
            self.pose_progress[index] = Some(SwitchMotion::Initial);
        }
        self.refresh_switch(reference, index);
    }
    fn refresh_switch(&mut self, reference: &crate::reference::LightingRecord, index: usize) {
        let c = &reference.controllers[index];
        let on = self.controllers[index].on
            && (c.electronics_source.is_none() || c.electronics_output > c.electronics_threshold);
        for lamp in &c.states {
            for id in &lamp.lights {
                let output = self.lights.iter_mut().find(|o| o.id == *id).unwrap();
                output.enabled = on && !lamp.broken;
                output.intensity = lamp.intensity;
            }
            select_material(&mut self.materials, lamp, on && !lamp.broken);
            for node in &lamp.working_nodes {
                self.visibility
                    .iter_mut()
                    .find(|o| o.node == *node)
                    .unwrap()
                    .visible = !lamp.broken;
            }
            for node in &lamp.broken_nodes {
                self.visibility
                    .iter_mut()
                    .find(|o| o.node == *node)
                    .unwrap()
                    .visible = lamp.broken;
            }
        }
    }
    pub fn visibility_outputs(&self) -> &[NodeVisibilityOutput] {
        &self.visibility
    }
    pub fn advance_fades(&mut self, reference: &crate::reference::LightingRecord, dt: f32) {
        if !dt.is_finite() || dt <= 0. {
            return;
        }
        for (index, control) in reference.controllers.iter().enumerate() {
            if control.kind == crate::reference::LightControllerKind::Switch {
                if let Some(motion) = self.pose_progress[index] {
                    let progress = match motion {
                        SwitchMotion::Initial => 0.,
                        SwitchMotion::Moving(progress) => progress + dt * 25.,
                    };
                    if progress >= 1. {
                        let target =
                            usize::from(self.controllers[index].on ^ control.reverse_switch);
                        for pose in &control.animated_poses {
                            let out = self.poses.iter_mut().find(|o| o.node == pose.node).unwrap();
                            out.local_pose.translation = pose.states[target].translation;
                            out.local_pose.rotation = pose.states[target].rotation;
                        }
                        self.pose_progress[index] = None;
                        continue;
                    }
                    let target = usize::from(self.controllers[index].on ^ control.reverse_switch);
                    for pose in &control.animated_poses {
                        let out = self.poses.iter_mut().find(|o| o.node == pose.node).unwrap();
                        let next = pose.states[target];
                        for i in 0..3 {
                            out.local_pose.translation[i] =
                                lerp(out.local_pose.translation[i], next.translation[i], progress);
                        }
                        let a = rapier3d::math::Rotation::from_array(out.local_pose.rotation);
                        let b = rapier3d::math::Rotation::from_array(next.rotation);
                        out.local_pose.rotation = a.lerp(b, progress).normalize().to_array();
                    }
                    self.pose_progress[index] = Some(SwitchMotion::Moving(progress));
                }
            }
        }
    }
    /// New road instances use the current phase and electronics output.
    pub fn road_lamp_output(&self, reference: &crate::reference::LightingRecord) -> LightOutput {
        let road = &reference.road_lamp;
        let controller = reference
            .controllers
            .iter()
            .find(|c| c.id == road.controller)
            .unwrap();
        LightOutput {
            id: road.lights[0],
            enabled: road.global_enabled
                && self.cycle.output().roadside_enabled
                && controller.electronics_output > road.electronics_threshold,
            intensity: controller.electronics_output,
        }
    }
    pub fn light_outputs(&self) -> &[LightOutput] {
        &self.lights
    }
    pub fn material_outputs(&self) -> &[LampMaterialOutput] {
        &self.materials
    }
    pub fn control_poses(&self) -> &[ControlPoseOutput] {
        &self.poses
    }
}
fn select_material(
    outputs: &mut [LampMaterialOutput],
    lamp: &crate::reference::LampStateReference,
    on: bool,
) {
    let material = if on {
        lamp.on_material.as_ref().or(lamp.off_material.as_ref())
    } else {
        lamp.off_material.as_ref()
    };
    if let Some(material) = material {
        for renderer in &lamp.renderers {
            let output = outputs
                .iter_mut()
                .find(|o| o.renderer == *renderer)
                .expect("prepared lamp slot");
            output.asset = material.asset;
            output.material_index = material.gltf_material;
        }
    }
}
/// Unity 2021.3 gamma conversion. HDR channels use the native 2.2 exponent.
pub fn source_linear(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else if value < 1. {
        ((value + 0.055) / 1.055).powf(2.4)
    } else {
        value.powf(2.2)
    }
}
/// Checked native byte attenuation table. The final texel is zero.
pub fn attenuation_table() -> [u8; 1024] {
    std::array::from_fn(|i| {
        if i == 1023 {
            return 0;
        }
        let s = i as f32 / 1024.;
        let mut value = 1. / (s * 25. + 1.);
        let start = f32::from_bits(0x3f23d70b);
        if s >= start {
            value *= 1. - (s - start) / f32::from_bits(0x3eb851ea);
        }
        (value * 255. + 0.5) as u8
    })
}

/// Keep the source repeat cookie phase bounded after a floating-origin shift.
pub fn directional_cookie_phase(
    basis: [[f32; 3]; 2],
    origin: [f64; 3],
    anchor: [f32; 3],
    size: f32,
) -> [f32; 2] {
    basis.map(|row| {
        let projected = (0..3)
            .map(|axis| f64::from(row[axis]) * (origin[axis] - f64::from(anchor[axis])))
            .sum::<f64>();
        (projected / f64::from(size)).rem_euclid(1.) as f32
    })
}

/// Native spotlight projection from the exported right-handed pose, without transform scale.
pub fn spot_cookie_matrix(
    pose: &crate::reference::Transform,
    range: f32,
    cot_half_angle: f32,
) -> [[f32; 4]; 4] {
    use rapier3d::math::{Rotation, Vector};
    let inverse = Rotation::from_array(pose.rotation).inverse();
    let scale = 1. / range;
    let projected_scale = (2. / cot_half_angle) * scale;
    [
        Vector::X,
        Vector::Y,
        Vector::Z,
        -Vector::from_array(pose.translation),
    ]
    .map(|column| {
        let local = inverse * column;
        // Export reflects Unity Z. X and Y keep their native cookie signs.
        [
            local.x * scale,
            local.y * scale,
            -local.z * scale,
            -local.z * projected_scale,
        ]
    })
}
