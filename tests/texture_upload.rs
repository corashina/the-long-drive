use the_long_slop::ownership::upload_receipt;

#[test]
fn texture_cpu_reservation_lasts_until_gpu_completion_across_forks() {
    let receipt = upload_receipt::UploadReceipt::default();
    let reservation = 48 * 1024 * 1024;
    assert_eq!(receipt.cpu_bytes(reservation, false), 0);
    assert_eq!(receipt.cpu_bytes(reservation, true), reservation);
    let fork = receipt.clone();
    assert!(fork.register());
    assert!(!receipt.register());
    assert_eq!(receipt.cpu_bytes(reservation, true), reservation);
    fork.complete();
    assert_eq!(receipt.cpu_bytes(reservation, true), 0);
    assert_eq!(fork.cpu_bytes(reservation, true), 0);
    assert!(!receipt.register());
}

use the_long_slop::ownership::texture_batch::*;
fn id(request: u64) -> BatchIdentity {
    BatchIdentity {
        session: 1,
        request,
    }
}
#[test]
fn pair_admission_and_promotion() {
    let mut queue = BatchQueue::default();
    assert_eq!(
        queue.admit([(id(1), 9), (id(2), 8)], 16, false),
        Err(Admission::Permanent)
    );
    assert_eq!(
        queue.admit([(id(1), 9), (id(2), 8)], 16, true),
        Err(Admission::Wait)
    );
    queue.admit([(id(1), 9), (id(2), 8)], 17, false).unwrap();
    assert_eq!(queue.start(1), vec![id(1)]);
    assert_eq!(queue.start(1), vec![id(2)]);
    assert!(queue.start(2).is_empty());
    assert_eq!(queue.jobs(), 2);
    queue.ready(id(2));
    assert!(!queue.submit(id(2)));
    queue.ready(id(1));
    assert!(queue.submit(id(1)));
    assert!(!queue.acknowledge(BatchIdentity {
        session: 2,
        request: 1
    }));
    assert!(queue.acknowledge(id(1)));
    assert_eq!(queue.release(id(1)), Some(9));
    assert!(queue.submit(id(2)));
}
#[test]
fn cancellation_keeps_loading_and_submitted_owners() {
    let mut queue = BatchQueue::default();
    queue
        .admit([(id(1), 10), (id(2), 20)], STAGING_LIMIT, false)
        .unwrap();
    queue.start(2);
    queue.ready(id(1));
    queue.submit(id(1));
    queue.cancel(1);
    assert_eq!(queue.bytes(), 30);
    assert_eq!(queue.jobs(), 1);
    assert_eq!(queue.release(id(1)), None);
    assert_eq!(queue.release(id(2)), None);
    queue.ready(id(2));
    assert_eq!(queue.release(id(2)), Some(20));
    queue.acknowledge(id(1));
    assert_eq!(queue.release(id(1)), Some(10));
    queue.admit([(id(3), 5)], STAGING_LIMIT, false).unwrap();
    assert!(!queue.acknowledge(id(1)));
    assert_eq!(queue.bytes(), 5);
}
#[test]
fn cap_is_permanent_even_when_other_work_can_finish() {
    let mut queue = BatchQueue::default();
    assert_eq!(
        queue.admit([(id(1), STAGING_LIMIT + 1)], usize::MAX, true),
        Err(Admission::Permanent)
    );
}

#[test]
fn shared_storage_survives_cancelled_session_until_last_owner() {
    let mut queue = BatchQueue::default();
    let other = BatchIdentity {
        session: 2,
        request: 2,
    };
    queue.admit([(id(1), 100), (other, 4)], 104, false).unwrap();
    queue.share(id(1), other, 96);
    queue.start(2);
    queue.cancel(1);
    assert_eq!(queue.bytes(), 104);
    queue.ready(id(1));
    assert_eq!(queue.release(id(1)), Some(4));
    assert_eq!(queue.bytes(), 100);
    assert!(!queue.owner(other).unwrap().cancelled);
    queue.ready(other);
    assert!(queue.submit(other));
    assert!(!queue.acknowledge(id(1)));
    assert_eq!(queue.bytes(), 100);
    queue.acknowledge(other);
    assert_eq!(queue.release(other), Some(100));
    assert_eq!(queue.bytes(), 0);
}
#[test]
#[ignore = "requires installed texture package"]
fn installed_pairs_fit_unchanged_staging_limit() {
    use the_long_slop::reference::LightingTextureRecord;
    let root = std::path::PathBuf::from(std::env::var("TEXTURE_BATCH_PACKAGE").unwrap());
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("reference/manifest.json")).unwrap())
            .unwrap();
    let mut reservations = Vec::new();
    let mut textures = 0;
    for group in [
        &manifest["lighting"]["textures"],
        &manifest["world"]["material_textures"],
        &manifest["world"]["terrain"]["textures"],
    ] {
        for value in group.as_array().unwrap() {
            let texture: LightingTextureRecord = serde_json::from_value(value.clone()).unwrap();
            for batch in texture.batches {
                assert!(batch.decoded_bytes <= 1024 * 1024);
                assert!(batch.region_count <= 32);
                reservations.push(reservation(&batch).unwrap());
            }
            textures += 1;
        }
    }
    let maximum = reservations.iter().copied().max().unwrap();
    assert!(maximum * 2 <= STAGING_LIMIT);
    assert_eq!(textures, 166);
    assert_eq!(reservations.len(), 503);
    println!(
        "textures={textures}, batches={}, maximum_owner={maximum}, maximum_pair={}, cap={STAGING_LIMIT}",
        reservations.len(),
        maximum * 2
    );
}
#[test]
fn refill_reserves_next_batch_without_resetting_promoted_owner() {
    let mut queue = BatchQueue::default();
    queue.admit([(id(1), 8), (id(2), 12)], 20, false).unwrap();
    queue.start(2);
    queue.ready(id(1));
    queue.ready(id(2));
    assert!(queue.submit(id(1)));
    assert_eq!(queue.refill(id(3), 10, 10), Err(Admission::Wait));
    queue.acknowledge(id(1));
    queue.release(id(1));
    assert_eq!(queue.owner(id(2)).unwrap().state, BatchState::Ready);
    assert_eq!(queue.refill(id(3), 10, 9), Err(Admission::Wait));
    assert_eq!(queue.bytes(), 12);
    queue.refill(id(3), 10, 10).unwrap();
    assert_eq!(queue.bytes(), 22);
    assert_eq!(queue.start(1), vec![id(3)]);
    assert!(queue.submit(id(2)));
    assert!(!queue.submit(id(3)));
    assert!(!queue.acknowledge(id(1)));
    queue.cancel(1);
    assert_eq!(queue.bytes(), 22);
    assert_eq!(queue.jobs(), 1);
    assert_eq!(queue.release(id(2)), None);
    assert_eq!(queue.release(id(3)), None);
    queue.ready(id(3));
    queue.release(id(3));
    assert_eq!(queue.bytes(), 12);
    queue.acknowledge(id(2));
    queue.release(id(2));
    assert_eq!(queue.bytes(), 0);
}
#[test]
fn refill_checks_combined_staging_before_launch() {
    let mut queue = BatchQueue::default();
    queue
        .admit([(id(1), STAGING_LIMIT - 1)], STAGING_LIMIT, false)
        .unwrap();
    assert_eq!(
        queue.refill(id(2), 2, STAGING_LIMIT),
        Err(Admission::Permanent)
    );
    assert!(queue.slots[1].is_none());
    queue.refill(id(2), 1, 1).unwrap();
    assert_eq!(queue.bytes(), STAGING_LIMIT);
}
#[test]
#[ignore = "requires installed texture package"]
fn installed_terrain_banks_preserve_source_bindings_without_duplicate_layers() {
    use the_long_slop::reference::{
        LightingTextureRecord, WorldSourceMaterial,
        texture_batch::{TextureArrayKey, texture_bank_groups},
    };
    let root = std::path::PathBuf::from(std::env::var("TEXTURE_BATCH_PACKAGE").unwrap());
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("reference/manifest.json")).unwrap())
            .unwrap();
    let materials: Vec<WorldSourceMaterial> =
        serde_json::from_value(manifest["world"]["terrain"]["materials"].clone()).unwrap();
    let textures: Vec<LightingTextureRecord> =
        serde_json::from_value(manifest["world"]["terrain"]["textures"].clone()).unwrap();
    let banks = texture_bank_groups(&materials, &textures).unwrap();
    assert_eq!(banks.len(), 5);
    assert_eq!(banks.iter().map(|(_, v)| v.len()).sum::<usize>(), 13);
    let bytes: usize = banks
        .iter()
        .map(|(key, layers)| key.layer_bytes().unwrap() * layers.len())
        .sum();
    assert_eq!(bytes, 50_681_156);
    let mut old_banks = Vec::new();
    for (index, material) in materials.iter().enumerate() {
        let groups = texture_bank_groups(std::slice::from_ref(material), &textures).unwrap();
        assert_eq!(groups.len(), [5, 4, 4][index]);
        for (key, layers) in groups {
            let ids: Vec<_> = layers.iter().map(|t| t.source.clone()).collect();
            if !old_banks.iter().any(|(k, old, _)| *k == key && *old == ids) {
                let bytes = key.layer_bytes().unwrap() * layers.len();
                old_banks.push((key, ids, bytes));
            }
        }
        for link in &material.textures {
            let original = textures.iter().find(|t| t.id == link.texture).unwrap();
            let (_, layers) = banks
                .iter()
                .find(|(k, _)| *k == TextureArrayKey::from(original))
                .unwrap();
            let bound = *layers.iter().find(|t| t.source == original.source).unwrap();
            // Reuse the same admitted record: source, payload receipts, mips, and sampler all remain exact.
            assert!(std::ptr::eq(original, bound));
            assert_eq!(bound.mip_data.sha256, original.mip_data.sha256);
            assert_eq!(bound.batches.len(), original.batches.len());
        }
    }
    let old: usize = old_banks.iter().map(|(_, _, bytes)| bytes).sum();
    assert_eq!(old, 85_633_680);
    println!(
        "terrain banks=5 layers=13 before={old} after={bytes} saved={}",
        old - bytes
    );
}
