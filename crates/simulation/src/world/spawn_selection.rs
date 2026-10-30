//! Source item/category selection. Missing source targets consume their selection draws.
use super::spawn_rules::*;
use super::{SpawnContext, UnityRandom};
use crate::reference::{ReferenceError, WorldReference};
pub fn select_named(
    reference: &WorldReference,
    name: &str,
    is_item: bool,
    random: &mut UnityRandom,
    context: SpawnContext,
) -> Result<Option<usize>, ReferenceError> {
    if is_item {
        return Ok(reference.catalog.items.iter().position(|i| i.name == name));
    }
    let Some(index) = reference
        .catalog
        .categories
        .iter()
        .position(|c| c.name == name)
    else {
        return Ok(None);
    };
    select_category(reference, index, random, context, 0)
}
fn select_category(
    reference: &WorldReference,
    index: usize,
    random: &mut UnityRandom,
    context: SpawnContext,
    depth: usize,
) -> Result<Option<usize>, ReferenceError> {
    if depth > reference.catalog.categories.len() {
        return Err(ReferenceError("Source category cycle".into()));
    }
    let Some(category) = reference.catalog.categories.get(index) else {
        return Ok(None);
    };
    let item_count = category.items.as_ref().map_or(0, Vec::len);
    let category_count = category.categories.as_ref().map_or(0, Vec::len);
    let item_total = category
        .item_chances
        .as_ref()
        .map_or(item_count as i32, |a| a.iter().map(|n| *n as i32).sum());
    let category_total = category
        .category_chances
        .as_ref()
        .map_or(category_count as i32, |a| a.iter().map(|n| *n as i32).sum());
    let draw = random.range_int(1, item_total + category_total + 1);
    let (values, weights, recursive) = if draw <= item_total {
        (&category.items, &category.item_chances, false)
    } else {
        (&category.categories, &category.category_chances, true)
    };
    let Some(values) = values else {
        return Ok(None);
    };
    if category.depend_on_car {
        // current_car is the source carTypeToCarID index, captured at the boundary.
        let car = context.current_car as usize;
        if random.range_int(0, 100) < 25 && car < values.len() {
            return Ok(Some(values[car] as usize));
        }
        let mut selected = 0;
        for _ in 0..10 {
            selected = random.range_int(0, values.len() as i32) as usize;
            if selected != car {
                break;
            }
        }
        return Ok(values.get(selected).map(|v| *v as usize));
    }
    let mut cumulative = 0;
    for (i, value) in values.iter().enumerate() {
        cumulative += weights.as_ref().map_or(1, |w| w[i] as i32);
        // The source does not subtract item_total in the category branch.
        if draw <= cumulative {
            return if recursive {
                select_category(reference, *value as usize, random, context, depth + 1)
            } else {
                Ok(Some(*value as usize))
            };
        }
    }
    Ok(Some(0))
}
pub fn select_pool(
    reference: &WorldReference,
    pool: &[crate::reference::WorldValue],
    random: &mut UnityRandom,
    context: SpawnContext,
) -> Result<Option<usize>, ReferenceError> {
    let mut total = 0.;
    for row in pool {
        total += number(fields(row)?, "chance")?;
    }
    let draw = random.range(0., total);
    let chances = pool
        .iter()
        .map(|row| fields(row).and_then(|f| number(f, "chance")).unwrap_or(0.));
    let Some(index) = pick_weighted(chances, draw) else {
        return Ok(None);
    };
    let row = fields(&pool[index])?;
    select_named(
        reference,
        text(row, "category")?,
        flag(row, "itemNameInsteadOfCategory")?,
        random,
        context,
    )
}
/// Source cumulative chances: the first row whose running sum exceeds `draw`.
pub fn pick_weighted(chances: impl Iterator<Item = f32>, draw: f32) -> Option<usize> {
    let mut cumulative = 0.;
    for (index, chance) in chances.enumerate() {
        cumulative += chance;
        if draw < cumulative {
            return Some(index);
        }
    }
    None
}
/// Source spawnInBoxScript.Spawn copy count from massScript.maxCountIfSpawnedInBox.
pub fn box_count(max: i32, random: &mut UnityRandom) -> i32 {
    if max > 0 {
        random.range_int(1, max + 1)
    } else {
        1
    }
}
