//! X02 part 3: guns. Reload and unload keep the rounds. A magazine swap saves and loads.
//! `roadmap_tests::weapons` fires each gun in the world.
use crate::interaction::guns::{AmmoItem, GunSnapshot, GunState, fr_allows};
use crate::interaction::spray::Ammo;
use crate::reference::{GunKind, GunReference, ItemId};

/// A large box of the gun's ammo type.
fn ammo_box(gun: &GunReference, count: f32) -> AmmoItem {
    AmmoItem {
        ammo: Ammo {
            count,
            maximum: 1000.,
            float: gun.ammo.float_or_int,
            kind: gun.ammo.r#type,
        },
        is_mag: false,
    }
}

/// An FR value where the gun's reload rule holds.
fn reload_fr(gun: &GunReference) -> f32 {
    [0., 1., gun.fr.reload.value, 0.5]
        .into_iter()
        .find(|v| fr_allows(&gun.fr.reload, *v))
        .expect("reload FR value")
}

#[test]
#[ignore = "requires the installed source export"]
fn reload_and_unload_keep_round_totals() {
    let reference = super::reference();
    let mut guns = GunState::new(&reference).unwrap();
    for (i, w) in reference.guns.weapons.iter().enumerate() {
        if matches!(w.kind, GunKind::Magazine { .. } | GunKind::None) {
            continue;
        }
        let item = ItemId(1_000_000 + i as u32);
        assert!(guns.bind(item, i, None, Some(0.)));
        guns.runtime_mut(item).unwrap().fr = reload_fr(w);
        let mut other = ammo_box(w, 100.);
        let total = |g: &GunState, o: &AmmoItem| g.runtime(item).unwrap().ammo.count + o.ammo.count;
        let start = total(&guns, &other);
        let mut loads = 0;
        while guns.can_reload(item, &other) {
            guns.reload(item, &mut other, false, false, [0.; 3]);
            assert_eq!(total(&guns, &other), start, "a reload keeps the rounds");
            loads += 1;
            assert!(loads < 1000, "each reload moves rounds");
        }
        let rt = guns.runtime(item).unwrap();
        assert_eq!(rt.ammo.count, rt.ammo.maximum, "reload fills the gun");
        if let GunKind::Revolver { chambers, .. } = &w.kind {
            assert_eq!(
                rt.chambers.count_ones(),
                (rt.ammo.count as u32).min(chambers.len() as u32),
                "one full chamber per round"
            );
        }
        while guns.can_unload(item, &other) {
            guns.reload(item, &mut other, true, false, [0.; 3]);
            assert_eq!(total(&guns, &other), start, "an unload keeps the rounds");
            loads += 1;
            assert!(loads < 2000, "each unload moves rounds");
        }
        let rt = guns.runtime(item).unwrap();
        assert_eq!(rt.ammo.count, 0., "unload empties the gun");
        if matches!(w.kind, GunKind::Revolver { .. }) {
            assert_eq!(rt.chambers, 0, "unload empties the chambers");
        }
    }
}

#[test]
#[ignore = "requires the installed source export"]
fn magazine_swap_saves_and_reloads() {
    let reference = super::reference();
    let (index, w) = reference
        .guns
        .weapons
        .iter()
        .enumerate()
        .find(|(_, w)| matches!(w.kind, GunKind::Magazine { .. }))
        .expect("magazine gun");
    let item = ItemId(1_000_000);
    let mut guns = GunState::new(&reference).unwrap();
    assert!(guns.bind(item, index, None, Some(5.)));
    let rt = guns.runtime_mut(item).unwrap();
    rt.fr = reload_fr(w);
    let (had, before) = (rt.has_mag, rt.ammo.count);
    let mut mag = AmmoItem {
        ammo: Ammo {
            count: 7.,
            maximum: rt.ammo.maximum,
            float: false,
            kind: w.ammo.r#type,
        },
        is_mag: true,
    };
    assert!(guns.can_reload(item, &mag), "a magazine fits its gun");
    let used = guns.reload(item, &mut mag, false, false, [0.; 3]).used;
    let rt = guns.runtime(item).unwrap();
    assert_eq!(used, !had, "a gun without a magazine uses the item up");
    assert!(rt.has_mag);
    assert_eq!(rt.ammo.count, 7.);
    if had {
        assert_eq!(
            mag.ammo.count, before,
            "the old magazine keeps the gun's rounds"
        );
    }
    let saved = guns.snapshot(item).expect("changed gun state");
    let json = serde_json::to_string(&saved).unwrap();
    let loaded: GunSnapshot = serde_json::from_str(&json).unwrap();
    let mut restored = GunState::new(&reference).unwrap();
    assert!(restored.bind(item, index, Some(loaded), Some(5.)));
    assert_eq!(restored.runtime(item).unwrap().snapshot(), saved);
}
