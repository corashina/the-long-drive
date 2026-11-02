use the_long_slop::reference::texture_batch::{MAX_BATCH_BYTES, TextureBatchDescriptor};
fn descriptor() -> TextureBatchDescriptor {
    serde_json::from_value(serde_json::json!({
        "source":{"file":"test","path_id":1},"width":2,"height":2,"mip_count":2,"decoded_bytes":20,
        "regions":[
            {"input":{"file":"reference/0.smip","bytes":32,"sha256":"a".repeat(64),"image_size":null},"mip":0,"row":0,"width":2,"rows":2,"texture_offset":0,"decoded_offset":0,"decoded_bytes":16},
            {"input":{"file":"reference/1.smip","bytes":20,"sha256":"b".repeat(64),"image_size":null},"mip":1,"row":0,"width":1,"rows":1,"texture_offset":16,"decoded_offset":16,"decoded_bytes":4}
        ]})).unwrap()
}
fn smip(width: u32, rows: u32, pixel: u8) -> Vec<u8> {
    let mut bytes = b"SMIP".to_vec();
    for word in [width, rows, 1] {
        bytes.extend(word.to_le_bytes());
    }
    bytes.resize(16 + (width * rows * 4) as usize, pixel);
    bytes
}
#[test]
fn reconstructs_grouped_mips_exactly() {
    let d = descriptor();
    d.validate().unwrap();
    let mut output = vec![0; 20];
    d.copy_input(0, &smip(2, 2, 7), &mut output).unwrap();
    d.copy_input(1, &smip(1, 1, 9), &mut output).unwrap();
    assert_eq!(output, [vec![7; 16], vec![9; 4]].concat());
}
#[test]
fn rejects_missing_duplicate_reordered_overlapping_and_bad_dimensions() {
    for change in 0..9 {
        let mut d = descriptor();
        match change {
            0 => {
                d.regions.pop();
            }
            1 => {
                d.regions.push(d.regions[0].clone());
            }
            2 => d.regions.reverse(),
            3 => d.regions[1].texture_offset = 0,
            4 => d.regions[1].decoded_offset = 0,
            5 => d.regions[0].width = 1,
            6 => d.regions[0].input.sha256 = "bad".into(),
            7 => d.regions[0].input.bytes = 16,
            _ => d.decoded_bytes = MAX_BATCH_BYTES + 1,
        }
        assert!(d.validate().is_err(), "case {change}");
    }
}
#[test]
fn rejects_empty_and_too_many_entries() {
    let mut d = descriptor();
    d.regions.clear();
    assert!(d.validate().is_err());
    let mut d = descriptor();
    d.regions = vec![d.regions[0].clone(); 33];
    assert!(d.validate().is_err());
}
#[test]
fn rejects_smip_headers_truncation_and_wrong_dimensions() {
    let d = descriptor();
    for input in [
        b"bad".to_vec(),
        smip(1, 4, 7),
        {
            let mut v = smip(2, 2, 7);
            v[0] = 0;
            v
        },
        {
            let mut v = smip(2, 2, 7);
            v.pop();
            v
        },
    ] {
        assert!(d.copy_input(0, &input, &mut [0; 20]).is_err());
    }
}
#[test]
fn bounds_descriptor_and_admitted_payload() {
    let bytes = serde_json::to_vec(&descriptor()).unwrap();
    TextureBatchDescriptor::parse(&bytes, 20).unwrap();
    assert!(TextureBatchDescriptor::parse(&bytes, 19).is_err());
    assert!(TextureBatchDescriptor::parse(&vec![b' '; 65537], 20).is_err());
}

#[test]
fn accepts_exact_payload_and_entry_limits() {
    let mut d = descriptor();
    d.width = 512;
    d.height = 512;
    d.mip_count = 1;
    d.decoded_bytes = MAX_BATCH_BYTES;
    d.regions.truncate(1);
    let r = &mut d.regions[0];
    r.width = 512;
    r.rows = 512;
    r.decoded_bytes = MAX_BATCH_BYTES;
    r.input.bytes = MAX_BATCH_BYTES as u32 + 16;
    d.validate().unwrap();
    d.width = 1;
    d.height = 32;
    d.decoded_bytes = 128;
    let template = d.regions[0].clone();
    d.regions = (0..32)
        .map(|row| {
            let mut r = template.clone();
            r.width = 1;
            r.rows = 1;
            r.row = row;
            r.decoded_bytes = 4;
            r.decoded_offset = row as usize * 4;
            r.texture_offset = r.decoded_offset;
            r.input.bytes = 20;
            r.input.file = format!("reference/{row}.smip");
            r
        })
        .collect();
    d.validate().unwrap();
}
#[test]
#[ignore = "requires staged installed texture package"]
fn installed_package_reconstructs_every_source_texture() {
    use the_long_slop::reference::{FileCatalog, LightingTextureRecord, ReferenceManifest};
    let root = std::path::PathBuf::from(std::env::var("TEXTURE_BATCH_PACKAGE").unwrap());
    let metadata = std::fs::read(root.join("reference/manifest.json")).unwrap();
    ReferenceManifest::from_runtime_json(&metadata).unwrap();
    let manifest: serde_json::Value = serde_json::from_slice(&metadata).unwrap();
    let mut catalog: FileCatalog =
        serde_json::from_value(manifest["file_catalog"].clone()).unwrap();
    catalog.admit().unwrap();
    let mut count = 0;
    for group in [
        &manifest["lighting"]["textures"],
        &manifest["world"]["material_textures"],
        &manifest["world"]["terrain"]["textures"],
    ] {
        for texture in group.as_array().unwrap() {
            let texture: LightingTextureRecord = serde_json::from_value(texture.clone()).unwrap();
            texture.validate_batch_receipts(&catalog).unwrap();
            let mut complete = Vec::new();
            for batch in &texture.batches {
                let bytes =
                    std::fs::read(root.join("runtime").join(&batch.descriptor.file)).unwrap();
                assert_eq!(bytes.len(), batch.descriptor.bytes as usize);
                let descriptor =
                    TextureBatchDescriptor::parse(&bytes, batch.decoded_bytes).unwrap();
                assert_eq!(descriptor.source, texture.source);
                assert_eq!(
                    (descriptor.width, descriptor.height, descriptor.mip_count),
                    (texture.width, texture.height, texture.mip_count)
                );
                let mut payload = vec![0; batch.decoded_bytes];
                for (index, region) in descriptor.regions.iter().enumerate() {
                    assert_eq!(catalog.file(&region.input.file).unwrap(), &region.input);
                    let input =
                        std::fs::read(root.join("runtime").join(&region.input.file)).unwrap();
                    descriptor.copy_input(index, &input, &mut payload).unwrap();
                }
                complete.extend(payload);
            }
            let original =
                std::fs::read(root.join("runtime").join(&texture.mip_data.path)).unwrap();
            assert_eq!(complete, original[16..], "{:?}", texture.source);
            count += 1;
        }
    }
    assert_eq!(count, 166);
}
