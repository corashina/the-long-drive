//! Transient condition state follows FStart, Refresh, and StartRandom2 caller order.
use super::spawn_contents::SpawnEvaluation;
use super::spawn_rules::*;
use super::{GeneratedId, Placement};
use crate::reference::{ReferenceError, SourceId};

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConditionState {
    pub parent: GeneratedId,
    pub source: SourceId,
    started: bool,
    set_random: bool,
    selected: i32,
    pub kind: String,
    pub states: [i32; 3],
    child_new_color: bool,
    pub color: [f32; 4],
}
impl ConditionState {
    pub fn retained_heap(&self) -> usize {
        super::contents::identity_heap(&self.parent)
            + self.source.file.capacity()
            + self.kind.capacity()
    }
}
impl SpawnEvaluation<'_> {
    fn condition_index(
        &mut self,
        parent: &Placement,
        record: SourceRecord<'_>,
    ) -> Result<usize, ReferenceError> {
        if let Some(index) = self
            .initialization
            .conditions
            .iter()
            .position(|c| c.parent == parent.id && c.source == *record.source)
        {
            return Ok(index);
        }
        let f = record.fields;
        let color = fields(value(f, "color")?)?;
        let index = self.initialization.conditions.len();
        self.initialization.conditions.push(ConditionState {
            parent: parent.id.clone(),
            source: record.source.clone(),
            started: flag(f, "started")?,
            set_random: flag(f, "setRandom")?,
            selected: integer(f, "slectedRandomTipus")?,
            kind: text(f, "tipus")?.to_owned(),
            states: [
                integer(f, "state")?,
                integer(f, "state2")?,
                integer(f, "state3")?,
            ],
            child_new_color: flag(f, "isChildnewColor")?,
            color: [
                number(color, "r")?,
                number(color, "g")?,
                number(color, "b")?,
                number(color, "a")?,
            ],
        });
        Ok(index)
    }
    pub(super) fn start_condition(
        &mut self,
        parent: &Placement,
        record: SourceRecord<'_>,
        override_limits: Option<[i32; 2]>,
        depth: usize,
    ) -> Result<(), ReferenceError> {
        condition_depth(depth)?;
        let i = self.condition_index(parent, record)?;
        if self.initialization.conditions[i].started {
            return Ok(());
        }
        self.initialization.conditions[i].started = true;
        let f = record.fields;
        if flag(f, "useRandomTipus")? {
            if !self.initialization.conditions[i].set_random {
                if self.random.range(0., 1.) <= number(f, "chanceToRandom")? {
                    let rows = array(f, "randomTipusok")?;
                    let total = rows.iter().try_fold(0., |n, row| {
                        Ok::<_, ReferenceError>(n + number(fields(row)?, "chance")?)
                    })?;
                    let draw = self.random.range(0., total);
                    let mut cumulative = 0.;
                    for (n, row) in rows.iter().enumerate() {
                        cumulative += number(fields(row)?, "chance")?;
                        if draw <= cumulative {
                            self.set_condition_random(parent, record, n as i32 + 1)?;
                            break;
                        }
                    }
                }
                for child in array(f, "randomChildsSet")? {
                    let child = self.rules.get(self.reference, value_link(child)?)?;
                    self.set_condition_random(
                        parent,
                        child,
                        self.initialization.conditions[i].selected,
                    )?;
                    self.refresh_condition(parent, child, depth + 1)?;
                }
            }
            let selected = self.initialization.conditions[i].selected;
            if selected != 0 {
                self.initialization.conditions[i].kind = text(
                    fields(&array(f, "randomTipusok")?[(selected - 1) as usize])?,
                    "tipus",
                )?
                .to_owned();
            }
        }
        if !flag(f, "loaded")?
            && flag(f, "randomStartState3InFStart")?
            && !self.initialization.conditions[i].set_random
        {
            self.initialization.conditions[i].states[2] =
                self.random.range_int(0, array(f, "GoList")?.len() as i32);
        }
        let start = self.random_start(parent, record)?;
        if override_limits.is_some() || start.start {
            self.initialization.conditions[i].states[1] =
                self.random.range_int(0, array(f, "MesList")?.len() as i32);
            self.initialization.conditions[i].states[2] =
                self.random.range_int(0, array(f, "GoList")?.len() as i32);
            self.condition_color(parent, record, depth + 1)?;
            let original = [start.min, start.max];
            let limits = match override_limits {
                Some([0, 0]) if original == [0, 0] => [0, 4],
                Some([0, 0]) => original,
                Some(v) => v,
                None => original,
            };
            self.condition_random2(
                parent,
                record,
                self.initialization.conditions[i].color,
                limits,
                depth + 1,
            )?;
        }
        self.refresh_condition(parent, record, depth + 1)
    }
    fn set_condition_random(
        &mut self,
        parent: &Placement,
        record: SourceRecord<'_>,
        selected: i32,
    ) -> Result<(), ReferenceError> {
        let i = self.condition_index(parent, record)?;
        if self.initialization.conditions[i].set_random {
            return Ok(());
        }
        self.initialization.conditions[i].set_random = true;
        self.initialization.conditions[i].selected = selected;
        let rows = array(record.fields, "randomTipusok")?;
        let row = rows
            .get((selected - 1).clamp(0, rows.len().saturating_sub(1) as i32) as usize)
            .ok_or_else(|| ReferenceError("Source SetRandom has no types".into()))?;
        let kind = text(fields(row)?, "tipus")?;
        if let Some(colors) = self.condition_colors(record, &kind)? {
            if colors.is_empty() && !flag(record.fields, "dontSetRandomColorTo1")? {
                self.initialization.conditions[i].color = [1.; 4];
            }
        }
        Ok(())
    }
    fn condition_colors<'b>(
        &'b self,
        record: SourceRecord<'b>,
        kind: &str,
    ) -> Result<Option<&'b [crate::reference::WorldValue]>, ReferenceError> {
        let main = self.rules.get(
            self.reference,
            &SourceId {
                file: "level1".into(),
                path_id: 64919,
            },
        )?;
        for row in array(main.fields, "conditionmaterials")? {
            let row = fields(row)?;
            if text(row, "tipus")? == kind {
                return Ok(Some(array(row, "colors")?));
            }
        }
        let _ = record;
        Ok(None)
    }
    fn condition_color(
        &mut self,
        parent: &Placement,
        record: SourceRecord<'_>,
        depth: usize,
    ) -> Result<(), ReferenceError> {
        condition_depth(depth)?;
        let i = self.condition_index(parent, record)?;
        if !self.initialization.conditions[i].started {
            self.start_condition(parent, record, None, depth + 1)?;
        }
        let kind = if flag(record.fields, "useOnlyMaterialTipusForMaterial")? {
            text(record.fields, "materialTipus")?.to_owned()
        } else {
            self.initialization.conditions[i].kind.clone()
        };
        // Copy the selected source color before mutating transient component state.
        let count = self
            .condition_colors(record, &kind)?
            .unwrap_or(array(record.fields, "colors")?)
            .len();
        let selected = self.random.range_int(0, count as i32);
        if count > 0 {
            let colors = self
                .condition_colors(record, &kind)?
                .unwrap_or(array(record.fields, "colors")?);
            let color = fields(&colors[selected as usize])?;
            self.initialization.conditions[i].color = [
                number(color, "r")?,
                number(color, "g")?,
                number(color, "b")?,
                number(color, "a")?,
            ];
        }
        Ok(())
    }
    fn condition_random2(
        &mut self,
        parent: &Placement,
        record: SourceRecord<'_>,
        color: [f32; 4],
        limits: [i32; 2],
        depth: usize,
    ) -> Result<(), ReferenceError> {
        condition_depth(depth)?;
        let f = record.fields;
        if flag(f, "isChild")? {
            return Ok(());
        }
        let i = self.condition_index(parent, record)?;
        self.initialization.conditions[i].color = color;
        let start = self.random_start(parent, record)?;
        let low = start.override_min.unwrap_or(limits[0]);
        let high = start.override_max.unwrap_or(limits[1]);
        self.initialization.conditions[i].states[0] = self.random.range_int(low, high + 1);
        self.refresh_condition(parent, record, depth + 1)?;
        for child in array(f, "childs")? {
            let child = self.rules.get(self.reference, value_link(child)?)?;
            if !flag(child.fields, "loaded")? && !flag(child.fields, "noInheritState")? {
                self.condition_random2(parent, child, color, limits, depth + 1)?;
            }
        }
        Ok(())
    }
    fn refresh_condition(
        &mut self,
        parent: &Placement,
        record: SourceRecord<'_>,
        depth: usize,
    ) -> Result<(), ReferenceError> {
        condition_depth(depth)?;
        let i = self.condition_index(parent, record)?;
        if !self.initialization.conditions[i].started {
            self.start_condition(parent, record, None, depth + 1)?;
        }
        let states = self.initialization.conditions[i].states;
        for child in array(record.fields, "childs")? {
            let child = self.rules.get(self.reference, value_link(child)?)?;
            let c = self.condition_index(parent, child)?;
            if flag(child.fields, "isChild")? {
                if !flag(record.fields, "noInheritState")? {
                    self.initialization.conditions[c].states[0] = states[0];
                    self.initialization.conditions[c].states[1] = states[1];
                    if !flag(child.fields, "randomStartState3InFStart")? {
                        self.initialization.conditions[c].states[2] = states[2];
                    }
                }
                if !flag(child.fields, "loaded")?
                    && self.initialization.conditions[c].child_new_color
                {
                    self.condition_color(parent, child, depth + 1)?;
                    self.initialization.conditions[c].child_new_color = false;
                }
                self.refresh_condition(parent, child, depth + 1)?;
            } else if flag(child.fields, "sameStateAsParent")? {
                self.initialization.conditions[c].states = states;
                self.refresh_condition(parent, child, depth + 1)?;
            }
        }
        Ok(())
    }
}
/// partconditionscript random start fields.
struct RandomStart {
    start: bool,
    min: i32,
    max: i32,
    override_min: Option<i32>,
    override_max: Option<i32>,
}
impl SpawnEvaluation<'_> {
    /// The random start fields of `record`. The picked start car takes its scene values.
    fn random_start(
        &self,
        parent: &Placement,
        record: SourceRecord<'_>,
    ) -> Result<RandomStart, ReferenceError> {
        if parent.id.layer == super::WorldLayer::Startup
            && parent.id.children.is_empty()
            && let Some(c) = self
                .reference
                .startup
                .get(parent.id.ordinal as usize)
                .and_then(|s| s.conditions.iter().find(|c| &c.source == record.source))
        {
            return Ok(RandomStart {
                start: c.random_start,
                min: c.random_min,
                max: c.random_max,
                override_min: c.override_min,
                override_max: c.override_max,
            });
        }
        let f = record.fields;
        Ok(RandomStart {
            start: flag(f, "randomStart")?,
            min: integer(f, "randomMin")?,
            max: integer(f, "randomMax")?,
            override_min: flag(f, "enableOverrideMin")?
                .then(|| integer(f, "overRideMin"))
                .transpose()?,
            override_max: flag(f, "enableOverrideMax")?
                .then(|| integer(f, "overRideMax"))
                .transpose()?,
        })
    }
}
fn condition_depth(depth: usize) -> Result<(), ReferenceError> {
    if depth >= 128 {
        Err(ReferenceError(
            "Source condition initialization cycle".into(),
        ))
    } else {
        Ok(())
    }
}
