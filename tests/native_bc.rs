use the_long_slop::reference::{
    FileCatalog, NativeBcFormat, ReferenceManifest, TextureRecord, TextureStorage,
    fixed_reservation::*,
};
fn texture() -> TextureRecord {
    serde_json::from_value(serde_json::json!({
        "slot":"_MainTex", "source":{"file":"sharedassets0.assets","path_id":284},
        "color_space":"srgb", "encoding":"rgba", "source_encoding":"rgba",
        "uv_channel":0,"scale":[1,1],"offset":[0,0],
        "sampler":{"filter":2,"wrap_u":0,"wrap_v":0,"aniso":1,"mip_bias":0,"mip_count":12},
        "storage":{"kind":"native_bc","format":"bc3","source_format":29,"width":2048,"height":2048,
            "mip_count":12,"image_count":1,"dimension":2,"mips_stripped":0,
            "input":{"file":"reference/texture-sharedassets0.assets-284-srgb.bc","bytes":5592432,
                "sha256":"0000000000000000000000000000000000000000000000000000000000000000","image_size":null}}
    })).unwrap()
}
fn catalog(texture: &TextureRecord) -> FileCatalog {
    let TextureStorage::NativeBc { input, .. } = &texture.storage else {
        unreachable!()
    };
    let mut result: FileCatalog = serde_json::from_value(serde_json::json!({
        "files":[input],"plans":[],"dependency_staging_reserve":1,
    }))
    .unwrap();
    result.admit().unwrap();
    result
}
#[test]
fn native_storage_gate_and_catalog_are_mandatory() {
    let original = texture();
    let files = catalog(&original);
    assert_eq!(primary_staging(&original, &files).unwrap(), 19_136_592);
    assert_eq!(original.payload_bytes().unwrap(), 5_592_432);
    assert_eq!(
        png_cpu(
            original.payload_bytes().unwrap(),
            primary_staging(&original, &files).unwrap()
        )
        .unwrap(),
        49_458_048
    );
    let value = serde_json::to_value(&original).unwrap();
    for (field, wrong) in [
        ("format", serde_json::json!("bc1")),
        ("source_format", serde_json::json!(4)),
        ("image_count", serde_json::json!(2)),
        ("dimension", serde_json::json!(3)),
        ("mips_stripped", serde_json::json!(1)),
        ("width", serde_json::json!(1024)),
        ("mip_count", serde_json::json!(11)),
    ] {
        let mut invalid = value.clone();
        invalid["storage"][field] = wrong;
        let invalid: TextureRecord = serde_json::from_value(invalid).unwrap();
        assert!(primary_staging(&invalid, &files).is_err(), "{field}");
    }
    for field in ["encoding", "source_encoding"] {
        let mut invalid = value.clone();
        invalid[field] = serde_json::json!("rgb");
        assert!(primary_staging(&serde_json::from_value(invalid).unwrap(), &files).is_err());
    }
    let mut invalid = value.clone();
    invalid["source"]["path_id"] = serde_json::json!(285);
    assert!(primary_staging(&serde_json::from_value(invalid).unwrap(), &files).is_err());
    for (field, wrong) in [
        ("bytes", serde_json::json!(5592431)),
        ("bytes", serde_json::json!(5592433)),
        ("sha256", serde_json::json!("1".repeat(64))),
    ] {
        let mut invalid = value.clone();
        invalid["storage"]["input"][field] = wrong;
        assert!(primary_staging(&serde_json::from_value(invalid).unwrap(), &files).is_err());
    }
    let mut missing = files.clone();
    missing.files.clear();
    missing.admit().unwrap();
    assert!(primary_staging(&original, &missing).is_err());
    let mut missing = value.clone();
    missing.as_object_mut().unwrap().remove("storage");
    assert!(serde_json::from_value::<TextureRecord>(missing).is_err());
    let mut obsolete = value;
    obsolete["mips"] = serde_json::json!([]);
    assert!(serde_json::from_value::<TextureRecord>(obsolete).is_err());
}
#[test]
fn block_rounding_keeps_rectangular_and_tiny_authored_tails() {
    assert_eq!(
        bc_payload(NativeBcFormat::Bc1, 1024, 512, 11).unwrap(),
        349544
    );
    assert_eq!(bc_payload(NativeBcFormat::Bc1, 7, 3, 3).unwrap(), 32);
    assert_eq!(bc_payload(NativeBcFormat::Bc3, 1, 2, 2).unwrap(), 32);
    assert_eq!(bc_payload(NativeBcFormat::Bc1, 1, 1, 1).unwrap(), 8);
    assert!(bc_payload(NativeBcFormat::Bc1, 1, 1, 2).is_err());
    assert!(bc_payload(NativeBcFormat::Bc3, 0, 16, 5).is_err());
}
#[test]
#[ignore = "requires installed native BC package"]
fn installed_native_primary_bytes() {
    let reference = ReferenceManifest::from_runtime_json(
        &std::fs::read("web/assets/reference/manifest.json").unwrap(),
    )
    .unwrap();
    let mut unique = std::collections::HashMap::new();
    let mut fixed = std::collections::HashMap::new();
    for material in &reference.rendering.materials {
        for texture in &material.properties.textures {
            unique.insert(texture.file(), texture);
            if (1..=7).contains(&material.asset.0) {
                fixed.insert(texture.file(), texture);
            }
        }
    }
    assert_eq!(unique.len(), 446);
    assert_eq!(
        unique
            .values()
            .filter(|t| matches!(t.storage, TextureStorage::NativeBc { .. }))
            .count(),
        346
    );
    assert_eq!(
        unique
            .values()
            .map(|t| t.payload_bytes().unwrap())
            .sum::<usize>(),
        580812608
    );
    assert_eq!(
        fixed
            .values()
            .map(|t| t.payload_bytes().unwrap())
            .sum::<usize>(),
        256296384
    );
}
