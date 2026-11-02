//! Source player needs, toilet actions, and death timing.

use crate::fluids::FluidContainer;
use crate::reference::{ConsumptionEffect, NeedValues, SurvivalReference};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ToiletInput {
    pub urinate: bool,
    pub burp_held: bool,
    pub burp_pressed: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ToiletOutput {
    pub burp: bool,
    pub fart: bool,
    pub defecated: bool,
    pub poured_liters: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NeedsOutput {
    pub needs: NeedValues,
    pub bladder_liters: f32,
    pub can_eat: bool,
    pub opacity: f32,
    pub dead: bool,
    pub death_seconds: f64,
    pub look_scale: f32,
    pub menu_due: bool,
}

pub struct SurvivalState {
    needs: NeedValues,
    pub bladder: FluidContainer,
    pending: ConsumptionEffect,
    maximum: NeedValues,
    show_seconds: f32,
    hide_seconds: f32,
    death_duration: f32,
    elapsed: f64,
    stats_shown_at: f64,
    food_hurt: bool,
    water_hurt: bool,
    hp_hurt: bool,
    dead: bool,
    death_elapsed: f64,
    valve: f32,
    emptied_bladder: bool,
    burp_gate: f32,
    fart_gate: f32,
}

impl SurvivalState {
    pub fn new(reference: &SurvivalReference) -> Self {
        Self {
            needs: reference.initial,
            bladder: FluidContainer::new(
                reference.bladder_capacity_liters,
                &reference.initial_bladder,
            ),
            pending: reference.initial_queue,
            maximum: reference.maximum,
            show_seconds: reference.food_stat_show_time,
            hide_seconds: reference.food_stat_hide_time,
            death_duration: reference.death_seconds,
            elapsed: 0.,
            stats_shown_at: -10.,
            food_hurt: false,
            water_hurt: false,
            hp_hurt: false,
            dead: reference.initial.hp <= 0.,
            death_elapsed: 0.,
            valve: 0.,
            emptied_bladder: false,
            burp_gate: 0.,
            fart_gate: 0.,
        }
    }

    pub fn queue(&mut self, effect: ConsumptionEffect) {
        if ![effect.food, effect.water, effect.bowel, effect.hp]
            .iter()
            .all(|v| v.is_finite())
        {
            return;
        }
        self.pending.food += effect.food;
        self.pending.water += effect.water;
        self.pending.bowel += effect.bowel;
        self.pending.hp += effect.hp;
    }

    pub fn pending(&self) -> ConsumptionEffect {
        self.pending
    }

    pub fn capture_save(&self) -> crate::save::NeedsSnapshot {
        crate::save::NeedsSnapshot {
            needs: self.needs,
            pending: self.pending,
            bladder: crate::fluids::FluidSnapshot::from(&self.bladder),
        }
    }

    /// Source LoadPlayer restores needs and queues without showing stats.
    pub fn restore_save(&mut self, snapshot: &crate::save::NeedsSnapshot) {
        self.needs = snapshot.needs;
        self.pending = snapshot.pending;
        self.bladder.restore_entries(snapshot.bladder.as_slice());
        self.food_hurt = self.needs.food <= 0.;
        self.water_hurt = self.needs.water <= 0.;
        self.hp_hurt = self.needs.hp < self.maximum.hp * 0.25;
        self.dead = self.needs.hp <= 0.;
        self.death_elapsed = 0.;
    }

    pub fn show_stats(&mut self) {
        self.stats_shown_at = self.elapsed;
    }

    /// Fractions use the source order: gas, oil, alcohol, water, blood, diesel.
    pub fn apply_drink(
        &mut self,
        reference: &SurvivalReference,
        fractions: [f32; 6],
        requested_liters: f32,
    ) {
        if !requested_liters.is_finite()
            || requested_liters <= 0.
            || !fractions.iter().all(|v| v.is_finite() && *v >= 0.)
        {
            return;
        }
        self.pending.water += requested_liters * fractions[3];
        self.pending.hp += requested_liters
            * (fractions[4] * reference.blood_to_hp_ratio + fractions[2] * reference.alcohol_heal
                - (fractions[0] + fractions[1] + fractions[5]) * reference.car_fluids_damage);
        self.convert_water_to_bladder(
            reference,
            requested_liters * fractions[2] * reference.alcohol_convert_to_piss,
        );
        self.show_stats();
    }

    pub fn damage_instant(&mut self, reference: &SurvivalReference, amount: f32) {
        if !amount.is_finite() || self.dead {
            return;
        }
        self.needs.hp = (self.needs.hp - amount).clamp(0., reference.maximum.hp);
        if self.needs.hp <= 0. {
            self.dead = true;
            self.death_elapsed = 0.;
        }
    }

    /// `survival` false keeps food and water full, as `survivalscript.Update`.
    pub fn advance(
        &mut self,
        reference: &SurvivalReference,
        dt: f32,
        temperature: f32,
        need_scale: f32,
        survival: bool,
    ) -> NeedsOutput {
        if !dt.is_finite()
            || dt < 0.
            || !temperature.is_finite()
            || !need_scale.is_finite()
            || need_scale < 0.
        {
            return self.output();
        }
        self.elapsed += f64::from(dt);
        if self.dead {
            self.death_elapsed += f64::from(dt);
        }
        let step = dt.min(1.);
        if survival {
            let cold = inverse_lerp(reference.def_temp, reference.cold_temp, temperature);
            let hot = inverse_lerp(reference.def_temp, reference.hot_temp, temperature);
            let food_loss = reference.food_loss
                * need_scale
                * lerp(
                    1.,
                    reference.cold_food_loss_modifier,
                    cold * lerp(1., reference.hot_food_loss_modifier, hot),
                );
            // The recovered update uses foodLoss for water as well.
            let water_loss = reference.food_loss
                * need_scale
                * lerp(
                    1.,
                    reference.cold_water_loss_modifier,
                    cold * lerp(1., reference.hot_water_loss_modifier, hot),
                );
            self.needs.food =
                (self.needs.food - food_loss * step).clamp(0., reference.maximum.food);
            self.needs.bowel += food_loss * reference.food_to_shit_ratio * step;
            self.convert_water_to_bladder(reference, water_loss * step);
            if !self.food_hurt && self.needs.food <= 0. {
                self.food_hurt = true;
                self.show_stats();
            }
            if !self.water_hurt && self.needs.water <= 0. {
                self.water_hurt = true;
                self.show_stats();
            }
        } else {
            self.needs.food = reference.maximum.food;
            self.needs.water = reference.maximum.water;
        }
        if self.hp_hurt {
            if self.needs.hp > reference.maximum.hp * 0.25 {
                self.hp_hurt = false;
            }
        } else if self.needs.hp < reference.maximum.hp * 0.25 {
            self.hp_hurt = true;
            self.show_stats();
        }
        let mut damage = 0.;
        if survival {
            if self.needs.water <= 0. {
                damage += reference.water_damage;
            } else {
                self.water_hurt = false;
            }
            if self.needs.food <= 0. {
                damage += reference.food_damage;
            } else {
                self.food_hurt = false;
            }
        }
        let fullness = inverse_lerp(reference.maximum.bowel, 0., self.needs.bowel).min(
            inverse_lerp(self.bladder.capacity_liters, 0., self.bladder.total()),
        );
        let heal = inverse_lerp(0., reference.maximum.water, self.needs.water).min(inverse_lerp(
            0.,
            reference.maximum.food,
            self.needs.food,
        )) * lerp(
            reference.max_healing_speed * reference.healing_toilet_full_modfiier,
            reference.max_healing_speed,
            fullness,
        );
        self.damage_instant(reference, damage * step * need_scale);
        if !self.dead {
            self.needs.hp =
                (self.needs.hp + heal * step * need_scale).clamp(0., reference.maximum.hp);
        }
        self.needs.bowel = self.needs.bowel.clamp(0., reference.maximum.bowel);

        let fraction = (dt * reference.delay_speed).min(1.);
        let applied = ConsumptionEffect {
            food: self.pending.food * fraction,
            water: self.pending.water * fraction,
            bowel: self.pending.bowel * fraction,
            hp: self.pending.hp * fraction,
        };
        self.pending.food -= applied.food;
        self.pending.water -= applied.water;
        self.pending.bowel -= applied.bowel;
        self.pending.hp -= applied.hp;
        self.eat_instant(reference, applied);
        self.output()
    }

    pub fn toilet_step(
        &mut self,
        reference: &SurvivalReference,
        input: ToiletInput,
        dt: f32,
        draws: [f32; 2],
        gate_seconds: f32,
        fluid_scale: f32,
    ) -> ToiletOutput {
        if !dt.is_finite()
            || dt < 0.
            || !fluid_scale.is_finite()
            || fluid_scale < 0.
            || !gate_seconds.is_finite()
            || gate_seconds < 0.
            || !draws.iter().all(|v| v.is_finite() && *v >= 0. && *v <= 1.)
        {
            return ToiletOutput::default();
        }
        self.burp_gate = (self.burp_gate - dt).max(0.);
        self.fart_gate = (self.fart_gate - dt).max(0.);
        let mut result = ToiletOutput::default();
        let target = if input.urinate && !self.emptied_bladder {
            1.
        } else {
            0.
        };
        self.valve = lerp(
            self.valve,
            target,
            (dt * reference.toilet.valve_gain).clamp(0., 1.),
        );
        if input.urinate {
            self.show_stats();
            if self.bladder.total() <= reference.toilet.empty_amount {
                self.emptied_bladder = true;
            }
        } else {
            self.emptied_bladder = false;
        }
        if self.valve > 0. && self.bladder.total() > 0. {
            let area =
                reference.toilet.radius * reference.toilet.radius * std::f32::consts::PI * 0.025;
            let poured = area * self.valve * 2. * dt * fluid_scale;
            self.bladder.change_without_mix(-poured);
            self.bladder.cleanup_one();
            result.poured_liters = poured;
        }
        if input.burp_held {
            self.show_stats();
        }
        if input.burp_pressed {
            if draws[0] < reference.toilet.fart_burp_ratio {
                if self.fart_gate <= 0. {
                    if self.needs.bowel >= reference.maximum.bowel * reference.toilet.bowel_fraction
                        && draws[1] > reference.toilet.fart_shit_ratio
                    {
                        self.needs.bowel -=
                            reference.maximum.bowel * reference.toilet.bowel_fraction;
                        result.defecated = true;
                    } else {
                        result.fart = true;
                    }
                    self.fart_gate = gate_seconds;
                }
            } else if self.burp_gate <= 0. {
                result.burp = true;
                self.burp_gate = gate_seconds;
            }
        }
        result
    }

    pub fn output(&self) -> NeedsOutput {
        let age = (self.elapsed - self.stats_shown_at) as f32;
        let opacity = if age >= self.show_seconds {
            0.
        } else if age < self.hide_seconds {
            1.
        } else {
            inverse_lerp(
                self.show_seconds,
                self.show_seconds - self.hide_seconds,
                age,
            )
        };
        let progress = if self.dead {
            (self.death_elapsed as f32 / self.death_duration).clamp(0., 1.)
        } else {
            0.
        };
        NeedsOutput {
            needs: self.needs,
            bladder_liters: self.bladder.total(),
            can_eat: !self.dead && self.needs.bowel + self.pending.bowel < self.maximum.bowel,
            opacity,
            dead: self.dead,
            death_seconds: self.death_elapsed,
            look_scale: if self.dead {
                lerp(0.8, 0., progress)
            } else {
                1.
            },
            menu_due: self.dead && progress >= 1.,
        }
    }

    fn convert_water_to_bladder(&mut self, reference: &SurvivalReference, amount: f32) {
        self.needs.water = (self.needs.water - amount).clamp(0., reference.maximum.water);
        let addition = (amount * reference.water_to_piss_ratio)
            .min((self.bladder.capacity_liters - self.bladder.total()).max(0.));
        self.bladder.change_one(addition, 3);
    }

    fn eat_instant(&mut self, reference: &SurvivalReference, effect: ConsumptionEffect) {
        let free_food = reference.maximum.food - self.needs.food;
        if effect.food <= free_food {
            self.needs.food += effect.food;
            self.needs.bowel = (self.needs.bowel + effect.bowel).clamp(0., reference.maximum.bowel);
        } else {
            self.needs.food = reference.maximum.food;
            self.needs.bowel = (self.needs.bowel + effect.bowel + effect.food - free_food)
                .clamp(0., reference.maximum.bowel);
        }
        let to_water = (reference.maximum.water - self.needs.water).min(effect.water);
        self.needs.water = (self.needs.water + to_water).clamp(0., reference.maximum.water);
        self.bladder.change_without_mix(effect.water - to_water);
        if effect.hp > 0. {
            if !self.dead {
                self.needs.hp = (self.needs.hp + effect.hp).clamp(0., reference.maximum.hp);
            }
        } else if effect.hp < 0. {
            self.damage_instant(reference, -effect.hp);
        }
        if effect.hp.abs() > 0.01 {
            self.show_stats();
        }
    }
}

fn inverse_lerp(a: f32, b: f32, value: f32) -> f32 {
    ((value - a) / (b - a)).clamp(0., 1.)
}

fn lerp(a: f32, b: f32, fraction: f32) -> f32 {
    a + (b - a) * fraction.clamp(0., 1.)
}
