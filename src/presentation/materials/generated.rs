//! Base material conversion for imported null material slots.
use super::*;
pub(super) fn from_base(base: StandardMaterial, mask: u32, reflected: bool) -> SourceMaterial {
    let mut surface = SourceSurface::default();
    surface.data.color = Vec4::from_array(base.base_color.to_linear().to_f32_array());
    surface.data.emission = Vec4::from_array(base.emissive.to_f32_array());
    surface.data.factors = Vec4::new(base.metallic, 1. - base.perceptual_roughness, 1., 0.5);
    surface.data.scales = Vec4::new(1., 1., 1., 0.);
    surface.data.flags.x = 1 << 11;
    surface.data.uv = [Vec4::new(1., 1., 0., 0.); 9];
    surface.state.reflected = reflected;
    surface.state.unlit = base.unlit;
    surface.state.standard = !base.unlit;
    surface.data.flags.z = u32::from(base.unlit);
    SourceMaterial {
        retained_color: None,
        preparation: default(),
        base,
        extension: SourceExtension {
            lod_fade: 1.,
            receiver_mask: mask,
            surface,
        },
    }
}
