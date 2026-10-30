//! Recovered source culling view transform.
pub type Matrix = [[f32; 4]; 4];
/// Culling inputs use source-world coordinates. Target world equals S * source world.
/// Right multiplication by S negates column2; camera/view space remains unchanged.
pub fn source_culling_view(mut target_view: Matrix) -> Matrix {
    for value in &mut target_view[2] {
        *value = -*value;
    }
    target_view
}
