//! Scene pixel sizing. Display coordinates and source projections stay native.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SceneScale(u32);

impl Default for SceneScale {
    fn default() -> Self {
        Self(100)
    }
}

impl SceneScale {
    /// Menu steps in percent of the native extent.
    pub const STEPS: [u32; 5] = [100, 85, 75, 67, 50];

    pub fn from_percent(percent: u32) -> Result<Self, &'static str> {
        if Self::STEPS.contains(&percent) {
            Ok(Self(percent))
        } else {
            Err("Scene resolution must be 100%, 85%, 75%, 67% or 50%")
        }
    }

    pub fn extent(self, native: [u32; 2]) -> [u32; 2] {
        native.map(|side| {
            if side == 0 {
                0
            } else {
                ((u64::from(side) * u64::from(self.0) / 100) as u32).max(1)
            }
        })
    }
}
