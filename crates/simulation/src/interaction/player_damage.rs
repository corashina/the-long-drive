//! Source `SDmgPlayer`: the damage that melee and gun hits do to other players.

/// `meleeweaponscript.playerDamageMultiplier`.
pub const MELEE_MULTIPLIER: f32 = 0.25;

/// The damage of this step for each hit player slot.
#[derive(Default)]
pub struct PlayerDamage {
    hits: Vec<(u8, f32)>,
}

impl PlayerDamage {
    /// The source sends one message for each hit collider. The port sends the sum for each player.
    pub fn add(&mut self, slot: u8, damage: f32) {
        match self.hits.iter_mut().find(|(s, _)| *s == slot) {
            Some((_, sum)) => *sum += damage,
            None => self.hits.push((slot, damage)),
        }
    }
    pub fn clear(&mut self) {
        self.hits.clear();
    }
    pub fn take(&mut self) -> impl Iterator<Item = (u8, f32)> + '_ {
        self.hits.drain(..)
    }
}
