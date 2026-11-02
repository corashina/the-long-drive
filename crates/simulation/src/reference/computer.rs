//! Checked computer (computerscript) and TV (displayscript) inputs.
use super::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComputerRecord {
    pub computer: ComputerReference,
    pub display: DisplayReference,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComputerReference {
    pub item: u32,
    pub prototype: SourceId,
    pub source: SourceId,
    pub x_size: u16,
    pub y_size: u16,
    /// CursorPosM: screen pixels per cursor column and row.
    pub cursor_step: [f32; 2],
    pub volume: f32,
    pub pitch_difference_chance: f32,
    pub pitch_difference: f32,
    pub screen: ComputerScreenReference,
}

/// The C64output canvas that the computer camera draws. Pixels are top-down; colors are source sRGB.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComputerScreenReference {
    pub size: [u16; 2],
    pub back: [f32; 4],
    pub front: [f32; 4],
    pub text: [f32; 4],
    pub cursor: [f32; 4],
    /// x, y, width, height of the Front panel. The Text fills it.
    pub front_rect: [f32; 4],
    pub cursor_size: [f32; 2],
    /// Glyph advance in whole pixels.
    pub advance: u8,
    pub line_height: f32,
    pub glyphs: GlyphAtlasReference,
}

/// Text glyphs of printable ASCII baked at the screen font size: one coverage cell per character.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GlyphAtlasReference {
    pub first: u8,
    pub cell: [u8; 2],
    /// Cell top-left relative to the glyph pen position at the line top.
    pub offset: [i8; 2],
    pub coverage: Vec<u8>,
}

impl GlyphAtlasReference {
    pub const COUNT: usize = 95;
    /// Coverage rows of a character. Characters outside the font have no glyph.
    pub fn glyph(&self, character: u8) -> Option<&[u8]> {
        let index = usize::from(character.checked_sub(self.first)?);
        let size = usize::from(self.cell[0]) * usize::from(self.cell[1]);
        (index < Self::COUNT).then(|| &self.coverage[index * size..(index + 1) * size])
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DisplayReference {
    pub item: u32,
    pub prototype: SourceId,
    pub source: SourceId,
    /// The node of displayscript.R.
    pub screen: SourceId,
    /// displayscript.R. It takes a private material and a runtime emission.
    pub renderer: SourceId,
    /// defMaterial `_EmissionColor` (source sRGB). OffMaterial has no emission.
    pub emission: [f32; 4],
    pub startup_on: bool,
    pub switch: DisplaySwitchReference,
}

/// The usablescript whose turnedEvent calls displayscript.Turned.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DisplaySwitchReference {
    pub source: SourceId,
    pub node: SourceId,
    pub clip: SourceId,
    pub volume: f32,
}

const COMPUTER_ITEM: u32 = 73;
const DISPLAY_ITEM: u32 = 152;

fn checked(ok: bool, field: &str) -> Result<(), ReferenceError> {
    if ok {
        Ok(())
    } else {
        Err(ReferenceError(format!("Invalid computer field {field}")))
    }
}

impl ComputerRecord {
    pub fn validate(&self, m: &ReferenceManifest) -> Result<(), ReferenceError> {
        let prototype = |item: u32, name: &str, source: &SourceId| {
            m.world
                .catalog
                .items
                .get(item as usize)
                .filter(|i| i.index == item && i.name == name)
                .and_then(|i| i.prototype.as_ref())
                .filter(|p| *p == source)
                .and_then(|p| m.world.prototypes.iter().find(|w| &w.source == p))
                .ok_or_else(|| ReferenceError(format!("Invalid computer owner {name}")))
        };
        let inside = |p: &WorldPrototypeReference, node: &SourceId| {
            p.nodes.iter().any(|n| &n.source == node)
        };
        let c = &self.computer;
        prototype(COMPUTER_ITEM, "gep01", &c.prototype)?;
        checked(
            c.item == COMPUTER_ITEM
                && c.x_size > 0
                && c.y_size > 0
                && finite(&c.cursor_step)
                && finite(&[c.volume, c.pitch_difference_chance, c.pitch_difference])
                && (0. ..=1.).contains(&c.volume)
                && (0. ..=1.).contains(&c.pitch_difference_chance)
                && c.pitch_difference >= 0.,
            "computer",
        )?;
        let s = &c.screen;
        let g = &s.glyphs;
        checked(
            s.size[0] > 0
                && s.size[1] > 0
                && finite(&s.back)
                && finite(&s.front)
                && finite(&s.text)
                && finite(&s.cursor)
                && finite(&s.front_rect)
                && finite(&s.cursor_size)
                && s.advance > 0
                && s.line_height.is_finite()
                && s.line_height > 0.
                && g.first == b' '
                && g.coverage.len()
                    == GlyphAtlasReference::COUNT * usize::from(g.cell[0]) * usize::from(g.cell[1]),
            "screen",
        )?;
        let d = &self.display;
        let p = prototype(DISPLAY_ITEM, "teve01", &d.prototype)?;
        let s = &d.switch;
        checked(
            d.item == DISPLAY_ITEM
                && inside(p, &d.screen)
                && inside(p, &s.node)
                && finite(&d.emission)
                && item_prototypes::clip_exists(m, &s.clip)
                && s.volume.is_finite()
                && (0. ..=1.).contains(&s.volume),
            "display",
        )
    }
}
