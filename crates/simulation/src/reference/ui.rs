use super::*;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FluidPresentation {
    pub has_name: bool,
    /// Source `nameText`: the `ls.w` index of the name. Valid when `has_name` is set.
    pub word: u32,
    pub no_text: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiSprite {
    pub role: String,
    pub source: SourceId,
    pub path: String,
    pub sha256: String,
    pub crop: [f32; 4],
    pub border: [f32; 4],
    pub size: [f32; 2],
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MenuPair {
    pub background: usize,
    pub frame: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiCanvas {
    pub resolution: [f32; 2],
    pub mode: u8,
    pub screen_match: u8,
    pub r#match: f32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiRect {
    pub source: SourceId,
    pub anchor_min: [f32; 2],
    pub anchor_max: [f32; 2],
    pub position: [f32; 2],
    pub size: [f32; 2],
    pub pivot: [f32; 2],
    pub scale: [f32; 2],
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiElement {
    pub source: SourceId,
    pub rect: [f32; 4],
    pub ancestry: Vec<UiRect>,
    pub canvas: UiCanvas,
    pub color: [f32; 4],
    pub font_size: f32,
    pub font: String,
    pub alignment: i32,
    pub text: String,
    pub sprite: Option<usize>,
    pub image_color: [f32; 4],
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiReference {
    pub crosshair_colors: CrosshairColors,
    pub need_sliders: HashMap<String, NeedSlider>,
    pub sprites: Vec<UiSprite>,
    pub pairs: Vec<MenuPair>,
    pub icons: HashMap<String, usize>,
    pub layouts: HashMap<String, UiElement>,
    pub overlay_color: [f32; 4],
    pub frame_color: [f32; 4],
}
/// Source `mainscript` old crosshair colors: `defCrosshairColor`, `canECrosshairColor`,
/// `canFCrosshairColor` and `canMMBCrosshairColor`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrosshairColors {
    pub default: [f32; 4],
    pub e: [f32; 4],
    pub f: [f32; 4],
    pub mmb: [f32; 4],
}
impl UiReference {
    /// The `sprites` index of a source sprite.
    pub fn sprite(&self, source: &SourceId) -> Option<u32> {
        self.sprites
            .iter()
            .position(|s| s.source == *source)
            .map(|i| i as u32)
    }
    pub fn validate(&self) -> Result<(), ReferenceError> {
        require(
            !self.sprites.is_empty() && !self.pairs.is_empty(),
            "UI assets",
        )?;
        for role in ["food", "water", "hp", "bladder", "bowel", "sleep"] {
            let slider = self
                .need_sliders
                .get(role)
                .ok_or_else(|| ReferenceError(format!("Missing need slider: {role}")))?;
            require(
                finite(&[slider.minimum, slider.maximum])
                    && slider.maximum > slider.minimum
                    && slider.direction <= 3,
                "need slider",
            )?;
            require(
                self.layouts.contains_key(&format!("need-{role}")),
                "need layout",
            )?;
        }
        for s in &self.sprites {
            require(
                hash(&s.sha256)
                    && s.path.starts_with("reference/")
                    && !s.path.contains("..")
                    && finite(&s.size)
                    && s.size.iter().all(|n| *n > 0.)
                    && finite(&s.crop)
                    && finite(&s.border),
                "UI sprite",
            )?;
        }
        for p in &self.pairs {
            require(
                p.background < self.sprites.len() && p.frame < self.sprites.len(),
                "UI menu pair",
            )?;
        }
        for role in [
            "default",
            "door",
            "slide",
            "slide-vertical",
            "switch",
            "push",
            "mount",
            "hose",
            "attach",
            "fill",
            "enter",
            "exit",
            "cap",
            "pickup",
            "dismount",
            "carrying",
            "carrying-medium",
            "carrying-heavy",
            "too-heavy",
        ] {
            require(
                self.icons
                    .get(role)
                    .is_some_and(|i| *i < self.sprites.len()),
                role,
            )?;
        }
        for role in [
            "start",
            "start-controls",
            "continue",
            "controls",
            "restart",
            "recover",
            "back",
            "use",
            "pickup",
            "mount",
            "fill",
            "carry",
            "help",
            "fluid",
            "reticle",
            "foot",
            "driving",
            "can-sleep",
            "sleep-hint",
            "cannot-sleep",
            "need-sleep",
            "need-sleep-background",
            "need-sleep-fill",
        ] {
            let e = self
                .layouts
                .get(role)
                .ok_or_else(|| ReferenceError(format!("Missing UI layout {role}")))?;
            require(
                finite(&e.rect)
                    && if role == "need-sleep-fill" {
                        e.rect[2] >= 0. && e.rect[3] >= 0.
                    } else {
                        e.rect[2] > 0. && e.rect[3] > 0.
                    }
                    && e.font_size.is_finite()
                    && e.font_size > 0.
                    && finite(&e.canvas.resolution)
                    && e.canvas.resolution.iter().all(|n| *n > 0.)
                    && e.canvas.mode <= 2
                    && e.canvas.screen_match <= 2
                    && finite(&e.color)
                    && e.color.iter().all(|n| (0.0..=1.0).contains(n))
                    && e.sprite.is_none_or(|i| i < self.sprites.len()),
                role,
            )?;
            for r in &e.ancestry {
                require(
                    finite(&r.anchor_min)
                        && finite(&r.anchor_max)
                        && finite(&r.position)
                        && finite(&r.size)
                        && finite(&r.scale)
                        && finite(&r.pivot),
                    "UI ancestry",
                )?;
            }
        }
        require(
            finite(&self.overlay_color) && finite(&self.frame_color),
            "UI colors",
        )?;
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NeedSlider {
    pub source: SourceId,
    pub minimum: f32,
    pub maximum: f32,
    pub direction: u8,
    pub whole_numbers: bool,
}
