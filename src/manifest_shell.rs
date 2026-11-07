//! The manifest fields that the browser interface reads.
use crate::interaction::labels::vehicle_name;
use crate::reference::{self, ReferenceManifest};

/// Manifest fields the browser shell reads.
#[derive(serde::Serialize)]
struct ManifestShell<'a> {
    app_id: u32,
    build_id: u32,
    source_hashes: &'a [reference::SourceHash],
    driving: &'a reference::DrivingReference,
    settings: &'a reference::SettingsReference,
    language: &'a reference::LanguageReference,
    ui: &'a reference::UiReference,
    sleep: &'a reference::SleepReference,
    audio: &'a reference::AudioRecord,
    content_base: &'a str,
    file_catalog: &'a reference::FileCatalog,
    items: &'a [reference::ItemRecord],
    world: WorldShell<'a>,
    clothing: ClothingShell<'a>,
}
/// The outfit defaults: the segments of each player character.
#[derive(serde::Serialize)]
struct ClothingShell<'a> {
    characters: Vec<CharacterShell<'a>>,
}
#[derive(serde::Serialize)]
struct CharacterShell<'a> {
    segments: &'a [reference::CharacterSegmentReference],
}
#[derive(serde::Serialize)]
struct WorldShell<'a> {
    draw_distance: &'a reference::DrawDistanceReference,
    mirrors: &'a reference::MirrorsReference,
    /// Start car names in export order. The Game mode start car list shows them.
    start_cars: Vec<String>,
}

/// The shell JSON of `reference`.
pub fn shell(reference: &ReferenceManifest) -> serde_json::Result<String> {
    let mut start_cars: Vec<String> = Vec::new();
    // Car09 has two start fields. The second one with a name shows "(2)".
    for startup in reference.world.startup.iter().filter(|s| s.start_car) {
        let name = vehicle_name(&startup.group).unwrap_or(&startup.group);
        let name = if start_cars.iter().any(|n| n == name) {
            format!("{name} (2)")
        } else {
            name.to_owned()
        };
        start_cars.push(name);
    }
    serde_json::to_string(&ManifestShell {
        app_id: reference.app_id,
        build_id: reference.build_id,
        source_hashes: &reference.source_hashes,
        driving: &reference.driving,
        settings: &reference.settings,
        language: &reference.language,
        ui: &reference.ui,
        sleep: &reference.sleep,
        audio: &reference.audio,
        content_base: &reference.content_base,
        file_catalog: &reference.file_catalog,
        items: &reference.items,
        world: WorldShell {
            draw_distance: &reference.world.draw_distance,
            mirrors: &reference.world.mirrors,
            start_cars,
        },
        clothing: ClothingShell {
            characters: reference
                .clothing
                .characters
                .iter()
                .map(|c| CharacterShell {
                    segments: &c.segments,
                })
                .collect(),
        },
    })
}
