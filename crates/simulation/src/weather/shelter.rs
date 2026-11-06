//! What covers the player from rain.
use serde::{Deserialize, Serialize};

/// The rain cover of the player. The value is the index of its rain loop blend.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum Shelter {
    #[default]
    None,
    /// The player is inside a live interior volume.
    Building,
    /// The player sits in a vehicle with a roof.
    Car,
}

impl Shelter {
    /// A roof seat gives `Car`. Else `inside` tells if the player is in a live interior volume.
    pub fn new(roof: bool, inside: impl FnOnce() -> bool) -> Self {
        if roof {
            Self::Car
        } else if inside() {
            Self::Building
        } else {
            Self::None
        }
    }
}

/// True when a vehicle has a roof. The reference has no roof flag.
/// Balanced vehicles are bikes. The Hummel Cabrio (root `Car08cFull`) is the open car.
pub fn vehicle_roof(root_name: &str, balanced: bool) -> bool {
    !balanced && crate::interaction::labels::vehicle_name(root_name) != Some("Hummel Cabrio")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_roofed_car_seat_gives_car_and_a_bike_seat_gives_none() {
        let shelter = |name, balanced| Shelter::new(vehicle_roof(name, balanced), || false);
        assert_eq!(shelter("Car02Full", false), Shelter::Car);
        assert_eq!(shelter("Bike01Full", true), Shelter::None);
        assert_eq!(shelter("Car08cFull", false), Shelter::None);
        assert_eq!(Shelter::new(false, || true), Shelter::Building);
    }
}
