//! Player-facing names for recovered interaction items.
use crate::reference::{ItemId, ItemKind, ReferenceManifest};

/// Game names of the vehicle types, by car key. A key is the lowercase car root name
/// or start car field without "Start", "full" and "_slide": Car02kFull, StartCar02k and car02k are car02k.
const CAR_NAMES: [(&str, &str); 21] = [
    ("car01", "Kestrel"),
    ("car02", "Volna"),
    ("car02k", "Volna Kombi"),
    ("car03", "Pip"),
    ("car03k", "Pip Kombi"),
    ("car04", "Sola"),
    ("car05", "Marten"),
    ("car06", "Bison"),
    ("car07", "Regent"),
    ("car08", "Hummel"),
    ("car08c", "Hummel Cabrio"),
    ("car09", "Falke"),
    ("car09s", "Falke"),
    ("car09p", "Falke Pickup"),
    ("car10", "Dobra"),
    ("car10k", "Dobra Kombi"),
    ("bus01", "Bus"),
    ("bus02", "Bus"),
    ("bus03", "Bus"),
    ("bike01", "Motorbike"),
    ("bike03", "Moped"),
];

/// The game name of a vehicle root name or start car field. Other names give None.
pub fn vehicle_name(name: &str) -> Option<&'static str> {
    let mut buffer = [0u8; 32];
    let key = buffer.get_mut(..name.len())?;
    key.copy_from_slice(name.as_bytes());
    key.make_ascii_lowercase();
    let mut key = &key[..];
    key = key.strip_prefix(b"start").unwrap_or(key);
    key = key.strip_suffix(b"_slide").unwrap_or(key);
    let (head, tail) = key
        .windows(4)
        .position(|w| w == b"full")
        .map_or((key, &[][..]), |i| (&key[..i], &key[i + 4..]));
    CAR_NAMES
        .iter()
        .find(|(car, _)| {
            let car = car.as_bytes();
            car.len() == head.len() + tail.len() && car.starts_with(head) && car.ends_with(tail)
        })
        .map(|(_, name)| *name)
}

pub fn display_name<'a>(
    reference: &'a ReferenceManifest,
    items: &'a crate::interaction::items::ItemStore,
    id: ItemId,
) -> &'a str {
    if let Some(name) = items.name(id).and_then(vehicle_name) {
        return name;
    }
    let Some(item) = reference.item(id) else {
        return items.name(id).unwrap_or("");
    };
    if let Some(tool) = reference.conditions.brushes.iter().find(|b| b.item == id) {
        return if tool.tool.cleaning_type == 1 {
            "Brush"
        } else {
            "Sponge"
        };
    }
    if reference.conditions.sprays.iter().any(|t| t.item == id) {
        return "Paint";
    }
    let label = match item.name.as_str() {
        "GarageDoorRightCollider" | "GarageDoorLeftCollider"
            if reference.item_mechanism(id).is_some() =>
        {
            Some("Garage door")
        }
        "TireDisztarcsa" => Some("Hubcap"),
        "SideMirror" | "LeftMirror" | "RightMirror" => Some("Side mirror"),
        "FrontMirror" => Some("Rearview mirror"),
        "Kesztyutarto" => Some("Glovebox"),
        "napellenzo" => Some("Sun visor"),
        _ => None,
    };
    if let Some(label) = label {
        return label;
    }
    if let Some(mechanism) = reference.item_mechanism(id)
        && mechanism.turn.is_none()
        && let Some(parent) = mechanism.parent_item
    {
        return display_name(reference, items, parent);
    }
    match item.kind {
        ItemKind::Food => item.name.as_str(),
        ItemKind::Decoration => "",
        ItemKind::Door => item.name.as_str(),
        ItemKind::Hood => "Hood",
        ItemKind::Trunk => "Trunk",
        ItemKind::Engine => "Engine",
        ItemKind::Radiator => "Radiator",
        ItemKind::Tire => "Tire",
        ItemKind::Rim => "Rim",
        ItemKind::Wheel => "Wheel",
        ItemKind::Seat => "Seat",
        ItemKind::Cap => {
            if reference
                .openings
                .iter()
                .any(|opening| opening.cap == id && opening.can_tune)
            {
                return "Water tap";
            }
            "Container cap"
        }
        ItemKind::Container => {
            if let Some(tank) = reference.item_tank(id) {
                if tank
                    .fluids
                    .iter()
                    .any(|entry| entry.fluid_type == 0 && entry.amount > 0.)
                {
                    "Fuel can"
                } else if tank
                    .fluids
                    .iter()
                    .any(|entry| entry.fluid_type == 1 && entry.amount > 0.)
                {
                    "Oil can"
                } else if tank
                    .fluids
                    .iter()
                    .any(|entry| entry.fluid_type == 3 && entry.amount > 0.)
                {
                    "Water container"
                } else {
                    "Container"
                }
            } else {
                "Container"
            }
        }
    }
}
