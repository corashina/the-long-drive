//! Source `settingsscript` Settings screen sliders and toggles, by `setting` field name.
use super::{ReferenceError, require};
use serde::{Deserialize, Serialize};

/// Source `slidersettingscript`. Values are slider units. The setting is `value * multiplier`.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingSlider {
    pub tab: SettingsTab,
    pub default: f32,
    /// `minValue`, `maxValue`: the clamp of every change.
    pub minimum: f32,
    pub maximum: f32,
    /// `sliderMinValue`, `sliderMaxValue`: the slider bar range.
    pub slider: [f32; 2],
    pub whole: bool,
    /// A slider move rounds to `round_digit`.
    pub round: bool,
    /// `roundWithIFToo`: every change rounds to `round_digit`.
    pub round_typed: bool,
    pub round_digit: i32,
    pub multiplier: f32,
    /// `onlyFromMainMenu`.
    pub menu_only: bool,
}

impl SettingSlider {
    /// The setting value of the default.
    pub fn value(&self) -> f32 {
        self.default * self.multiplier
    }
    pub fn contains(&self, setting: f32) -> bool {
        (self.minimum * self.multiplier..=self.maximum * self.multiplier).contains(&setting)
    }
}

/// Source `togglesetting`. A child turns off and locks while its parent is off.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingToggle {
    pub tab: SettingsTab,
    pub default: bool,
    pub children: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingsTab {
    Graphics,
    Controls,
    Misc,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsReference {
    pub sliders: SettingSliders,
    pub toggles: std::collections::BTreeMap<String, SettingToggle>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingSliders {
    #[serde(rename = "FFPSLimit")]
    pub fps_limit: SettingSlider,
    #[serde(rename = "FMouseSensXY")]
    pub mouse_xy: SettingSlider,
    #[serde(rename = "FMouseSensX")]
    pub mouse_x: SettingSlider,
    #[serde(rename = "FMouseSensY")]
    pub mouse_y: SettingSlider,
    #[serde(rename = "FMouseSensZ")]
    pub scroll: SettingSlider,
    #[serde(rename = "FMouseMove")]
    pub object_move: SettingSlider,
    #[serde(rename = "FMouseRot")]
    pub object_rotation: SettingSlider,
    #[serde(rename = "FKeyLookSens")]
    pub key_look: SettingSlider,
    #[serde(rename = "FKeySteerSens")]
    pub key_steer: SettingSlider,
    #[serde(rename = "FSound")]
    pub sound: SettingSlider,
    #[serde(rename = "FFieldOfViewFoot")]
    pub fov_foot: SettingSlider,
    #[serde(rename = "FFieldOfViewCar")]
    pub fov_car: SettingSlider,
    #[serde(rename = "FCrosshairScale")]
    pub crosshair_scale: SettingSlider,
}

impl SettingsReference {
    /// The toggle default. Panics on a name that `validate` did not check.
    pub fn toggle(&self, name: &str) -> bool {
        self.toggles[name].default
    }

    pub(super) fn validate(&self) -> Result<(), ReferenceError> {
        let s = &self.sliders;
        for slider in [
            &s.fps_limit,
            &s.mouse_xy,
            &s.mouse_x,
            &s.mouse_y,
            &s.scroll,
            &s.object_move,
            &s.object_rotation,
            &s.key_look,
            &s.key_steer,
            &s.sound,
            &s.fov_foot,
            &s.fov_car,
            &s.crosshair_scale,
        ] {
            require(
                [slider.minimum, slider.maximum, slider.multiplier]
                    .iter()
                    .chain(&slider.slider)
                    .all(|v| v.is_finite())
                    && slider.multiplier > 0.
                    && (slider.minimum..=slider.maximum).contains(&slider.default)
                    && slider.slider[0] <= slider.slider[1],
                "settings slider",
            )?;
        }
        for name in PlayerSettings::TOGGLES {
            require(self.toggles.contains_key(*name), "settings toggle")?;
        }
        require(
            self.toggles.values().all(|toggle| {
                toggle
                    .children
                    .iter()
                    .all(|child| self.toggles.contains_key(child))
            }),
            "settings toggle child",
        )
    }
}

/// Settings the simulation takes from the browser (source `settingsscript.S`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerSettings {
    /// `FMouseSensXY`, `FMouseSensX`, `FMouseSensY`.
    pub mouse: [f32; 3],
    /// `FMouseSensZ`.
    pub scroll: f32,
    /// `FMouseMove`, `FMouseRot`.
    pub object_move: f32,
    pub object_rotation: f32,
    /// `FKeyLookSens`.
    pub key_look: f32,
    /// `FKeySteerSens`.
    pub key_steer: f32,
    /// `FFieldOfViewFoot`, `FFieldOfViewCar`.
    pub fov: [f32; 2],
    /// `BInvertMouseSensX`, `Y`, `Z`, `BInvertMouseMove`, `BInvertMouseRot`.
    pub invert: [bool; 5],
    /// `BWalkHeadBob`, `BDriveHeadBob`.
    pub head_bob: [bool; 2],
    /// `inputs[inventory1].GetName() == inputs[gear1].GetName()`: a seated clutch blocks the
    /// inventory keys.
    pub inventory_gear_keys: bool,
}

impl PlayerSettings {
    const TOGGLES: &[&str] = &[
        "BInvertMouseSensX",
        "BInvertMouseSensY",
        "BInvertMouseSensZ",
        "BInvertMouseMove",
        "BInvertMouseRot",
        "BWalkHeadBob",
        "BDriveHeadBob",
    ];

    pub fn defaults(manifest: &super::ReferenceManifest) -> Self {
        let reference = &manifest.settings;
        let s = &reference.sliders;
        let t = |i: usize| reference.toggle(Self::TOGGLES[i]);
        // Source `oneInput.GetName`: the first key that is not None.
        let name = |action: &str| {
            manifest
                .driving
                .bindings
                .iter()
                .find(|b| b.action == action)
                .and_then(|b| b.keys.iter().flatten().next())
        };
        Self {
            mouse: [s.mouse_xy.value(), s.mouse_x.value(), s.mouse_y.value()],
            scroll: s.scroll.value(),
            object_move: s.object_move.value(),
            object_rotation: s.object_rotation.value(),
            key_look: s.key_look.value(),
            key_steer: s.key_steer.value(),
            fov: [s.fov_foot.value(), s.fov_car.value()],
            invert: [t(0), t(1), t(2), t(3), t(4)],
            head_bob: [t(5), t(6)],
            inventory_gear_keys: name("inventory1") == name("gear1"),
        }
    }

    pub fn valid(&self, reference: &SettingsReference) -> bool {
        let s = &reference.sliders;
        s.mouse_xy.contains(self.mouse[0])
            && s.mouse_x.contains(self.mouse[1])
            && s.mouse_y.contains(self.mouse[2])
            && s.scroll.contains(self.scroll)
            && s.object_move.contains(self.object_move)
            && s.object_rotation.contains(self.object_rotation)
            && s.key_look.contains(self.key_look)
            && s.key_steer.contains(self.key_steer)
            && s.fov_foot.contains(self.fov[0])
            && s.fov_car.contains(self.fov[1])
    }

    /// Source `fpscontroller.GetLook` mouse factors: X, then Y.
    pub fn look(&self) -> [f32; 2] {
        let sign = |inverted: bool| if inverted { -1. } else { 1. };
        [
            self.mouse[0] * self.mouse[1] * sign(self.invert[0]),
            self.mouse[0] * self.mouse[2] * sign(self.invert[1]),
        ]
    }

    /// Source `fpscontroller.scroll` factor. Slides and tank caps take `scroll` without the invert.
    pub fn player_scroll(&self) -> f32 {
        if self.invert[2] {
            -self.scroll
        } else {
            self.scroll
        }
    }

    /// Source `objMoveSens`, `objRotSens`.
    pub fn object(&self) -> [f32; 2] {
        [
            if self.invert[3] {
                -self.object_move
            } else {
                self.object_move
            },
            if self.invert[4] {
                -self.object_rotation
            } else {
                self.object_rotation
            },
        ]
    }
}
