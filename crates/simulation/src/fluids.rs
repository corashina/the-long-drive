//! Six-fluid containers owned by the item store.

use crate::reference::TankReference;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FluidEntry {
    pub fluid_type: u8,
    pub amount: f32,
}

pub fn validate_fluid_entries(entries: &[FluidEntry]) -> bool {
    let mut seen = [false; 6];
    entries.len() <= 6
        && entries.iter().all(|entry| {
            let index = entry.fluid_type as usize;
            if index >= 6 || seen[index] || !entry.amount.is_finite() {
                return false;
            }
            seen[index] = true;
            true
        })
}

/// Fixed ordered state for frame change capture. Its JSON form is an entry list.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FluidSnapshot {
    entries: [FluidEntry; 6],
    len: u8,
}
impl FluidSnapshot {
    pub fn as_slice(&self) -> &[FluidEntry] {
        &self.entries[..self.len as usize]
    }
    pub fn as_mut_slice(&mut self) -> &mut [FluidEntry] {
        &mut self.entries[..self.len as usize]
    }
}
impl From<&FluidContainer> for FluidSnapshot {
    fn from(container: &FluidContainer) -> Self {
        let mut snapshot = Self {
            len: container.order_len,
            ..Self::default()
        };
        for (slot, entry) in snapshot.entries.iter_mut().zip(container.entries()) {
            *slot = entry;
        }
        snapshot
    }
}
impl Serialize for FluidSnapshot {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.as_slice().serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for FluidSnapshot {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Entries;
        impl<'de> serde::de::Visitor<'de> for Entries {
            type Value = FluidSnapshot;
            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("at most six unique finite fluid entries")
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> Result<Self::Value, A::Error> {
                let mut snapshot = FluidSnapshot::default();
                while let Some(entry) = sequence.next_element::<FluidEntry>()? {
                    if snapshot.len as usize == snapshot.entries.len() {
                        return Err(serde::de::Error::invalid_length(7, &self));
                    }
                    snapshot.entries[snapshot.len as usize] = entry;
                    snapshot.len += 1;
                }
                if !validate_fluid_entries(snapshot.as_slice()) {
                    return Err(serde::de::Error::custom("Invalid fluid snapshot entries"));
                }
                Ok(snapshot)
            }
        }
        deserializer.deserialize_seq(Entries)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FluidMixture {
    pub quantities_liters: [f32; 6],
}

impl FluidMixture {
    pub fn total(&self) -> f32 {
        self.quantities_liters.iter().sum()
    }

    pub fn amount(&self, index: usize) -> f32 {
        self.quantities_liters[index]
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FluidContainer {
    pub capacity_liters: f32,
    mixture: FluidMixture,
    order: [u8; 6],
    order_len: u8,
}

impl FluidContainer {
    pub fn new(capacity_liters: f32, entries: &[FluidEntry]) -> Self {
        let mut container = Self {
            capacity_liters,
            mixture: FluidMixture::default(),
            order: [0; 6],
            order_len: 0,
        };
        container.restore_entries(entries);
        container
    }

    pub fn entries(&self) -> impl Iterator<Item = FluidEntry> + '_ {
        self.order[..self.order_len as usize]
            .iter()
            .map(|&fluid_type| FluidEntry {
                fluid_type,
                amount: self.mixture.amount(fluid_type as usize),
            })
    }

    /// Source GetAmount sums retained entries in list order.
    pub fn total(&self) -> f32 {
        self.entries().map(|entry| entry.amount).sum()
    }

    /// Finite derived volume for physics and presentation only.
    pub fn finite_total(&self) -> f32 {
        let total = self.total();
        if total.is_finite() { total } else { 0. }
    }

    pub fn mixture(&self) -> &FluidMixture {
        &self.mixture
    }

    pub fn restore_entries(&mut self, entries: &[FluidEntry]) {
        self.mixture = FluidMixture::default();
        self.order_len = 0;
        for entry in entries {
            self.remember(entry.fluid_type as usize);
            self.mixture.quantities_liters[entry.fluid_type as usize] = entry.amount;
        }
    }

    pub fn bound_engine_drain(&mut self, prior: [f32; 6]) {
        for (amount, before) in self.mixture.quantities_liters.iter_mut().zip(prior) {
            *amount = amount.max(0.).min(before.max(0.));
        }
    }

    fn remember(&mut self, index: usize) {
        if self.order[..self.order_len as usize].contains(&(index as u8)) {
            return;
        }
        self.order[self.order_len as usize] = index as u8;
        self.order_len += 1;
    }

    pub fn change_one(&mut self, amount: f32, index: usize) -> f32 {
        let free = self.capacity_liters - self.total();
        let changed = if amount < f32::MIN {
            f32::MIN
        } else if amount > free {
            free
        } else {
            amount
        };
        if self.order[..self.order_len as usize].contains(&(index as u8)) {
            self.mixture.quantities_liters[index] += changed;
            return changed;
        }
        if changed <= 0. {
            return 0.;
        }
        self.remember(index);
        self.mixture.quantities_liters[index] = changed;
        changed
    }

    /// Returns the signed change before source tank cleanup.
    pub fn change_without_mix(&mut self, amount: f32) -> f32 {
        let mut changed = 0.;
        for position in 0..self.order_len as usize {
            let index = self.order[position] as usize;
            let fraction = self.mixture.quantities_liters[index] / self.total();
            let change = amount * fraction;
            self.mixture.quantities_liters[index] += change;
            changed += change;
        }
        changed
    }

    pub fn cleanup_one(&mut self) {
        if let Some(position) = self.order[..self.order_len as usize]
            .iter()
            .position(|&index| self.mixture.quantities_liters[index as usize] <= 0.)
        {
            let index = self.order[position] as usize;
            self.mixture.quantities_liters[index] = 0.;
            self.order
                .copy_within(position + 1..self.order_len as usize, position);
            self.order_len -= 1;
        }
    }
}

impl From<&TankReference> for FluidContainer {
    fn from(reference: &TankReference) -> Self {
        Self::new(reference.capacity_liters, &reference.fluids)
    }
}

/// Tanks of one engine step. A missing tank skips its rules.
pub struct VehicleFluids<'a> {
    pub fuel: Option<&'a mut FluidContainer>,
    pub sump: Option<&'a mut FluidContainer>,
    pub coolant: Option<&'a mut FluidContainer>,
}

/// Both helpers receive amounts after caller sleep scaling.
pub fn transfer(source: &mut FluidContainer, destination: &mut FluidContainer, liters: f32) -> f32 {
    drink_transfer(source, destination, liters, false, false)
}

/// Returns the ordered debit before the source tank update removes one entry.
pub fn spill(container: &mut FluidContainer, liters: f32) -> f32 {
    if !liters.is_finite() || liters <= 0. {
        return 0.;
    }
    let removed = -container.change_without_mix(-liters);
    container.cleanup_one();
    removed
}

/// Source drink pour uses the remaining source fraction for each fluid.
pub fn drink_transfer(
    source: &mut FluidContainer,
    destination: &mut FluidContainer,
    liters: f32,
    filter_water: bool,
    filter_blood: bool,
) -> f32 {
    if !liters.is_finite() || liters <= 0. {
        return 0.;
    }
    let mut debited = 0.;
    for position in 0..source.order_len as usize {
        let index = source.order[position] as usize;
        let total = source.total();
        let fraction = source.mixture.quantities_liters[index] / total;
        let accepted = destination.change_one(liters * fraction, index);
        source.change_one(-accepted, index);
        debited += accepted;
        if (index == 3 && filter_water) || (index == 4 && filter_blood) {
            destination.change_one(-liters * fraction, index);
        }
    }
    destination.cleanup_one();
    source.cleanup_one();
    debited
}

/// Particle contact adds emitted fluid to the bladder without another source debit.
pub fn contact_drink_transfer(
    destination: &mut FluidContainer,
    source: &FluidContainer,
    fractions: [f32; 6],
    liters: f32,
    filter_water: bool,
    filter_blood: bool,
) {
    if !liters.is_finite() || liters <= 0. {
        return;
    }
    for position in 0..source.order_len as usize {
        let index = source.order[position] as usize;
        let fraction = fractions[index];
        let amount = liters * fraction;
        destination.change_one(amount, index);
        if (index == 3 && filter_water) || (index == 4 && filter_blood) {
            destination.change_one(-amount, index);
        }
    }
    destination.cleanup_one();
}

/// The source `canRun` takes the smallest supplied-to-required fluid fraction.
pub fn suitability(container: &FluidContainer, required: &FluidMixture) -> f32 {
    let total = container.total();
    let required_total = required.total();
    if total <= 0.0 || required_total <= 0.0 {
        return 0.0;
    }
    let mut result: f32 = 1.0;
    for index in 0..6 {
        let amount = required.amount(index);
        if amount > 0.0 {
            result =
                result.min((container.mixture.amount(index) / total) / (amount / required_total));
        }
    }
    result.clamp(0.0, 1.0)
}
