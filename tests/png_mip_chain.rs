use the_long_slop::reference::{
    FileCatalog, FileReceipt, ReferenceManifest, TextureMip, fixed_reservation::*,
};
#[test]
fn sequential_png_reserves_base_reader_with_larger_later_input() {
    let mips = vec![
        TextureMip {
            file: "base.png".into(),
            width: 4096,
            height: 4096,
        },
        TextureMip {
            file: "tail.png".into(),
            width: 2048,
            height: 2048,
        },
    ];
    let base = 64 * 1024;
    let later = 4 * 1024 * 1024;
    let staging = png_chain_staging(&mips, 2, &catalog(&mips, &[base, later])).unwrap();
    assert!(staging >= base + png_staging(later, 2048).unwrap());
    let pixels = png_pixels(&mips).unwrap();
    let cpu = png_cpu(pixels, staging).unwrap();
    // Base RGB+RGBA conversion precedes final-chain allocation.
    assert!(cpu >= 4096 * 4096 * 7 + 2 * base);
    // Later conversion overlaps final output and retained encoded base.
    assert!(cpu >= pixels + 2048 * 2048 * 7 + base + png_staging(later, 2048).unwrap());
    let mut missing = catalog(&mips, &[base, later]);
    missing.files.pop();
    missing.admit().unwrap();
    assert!(png_chain_staging(&mips, 2, &missing).is_err());
}
#[test]
#[ignore = "requires the installed source export"]
fn installed_png_chains_fit_unchanged_single_job_caps() {
    let reference = ReferenceManifest::from_runtime_json(
        &std::fs::read("web/assets/reference/manifest.json").unwrap(),
    )
    .unwrap();
    let mut count = 0;
    for texture in reference
        .rendering
        .materials
        .iter()
        .flat_map(|m| &m.properties.textures)
        .chain(&reference.plates.material.textures)
    {
        let staging = primary_staging(texture, &reference.file_catalog).unwrap();
        let cpu = png_cpu(texture.payload_bytes().unwrap(), staging).unwrap();
        assert!(staging <= 32 * 1024 * 1024, "{}: {staging}", texture.file());
        assert!(cpu <= 256 * 1024 * 1024, "{}: {cpu}", texture.file());
        count += 1;
    }
    for texture in &reference.render_lighting.textures {
        let staging = png_chain_staging(
            &texture.mips,
            texture.sampler.mip_count,
            &reference.file_catalog,
        )
        .unwrap();
        let cpu = png_cpu(png_pixels(&texture.mips).unwrap(), staging).unwrap();
        assert!(staging <= 32 * 1024 * 1024);
        assert!(cpu <= 256 * 1024 * 1024);
        count += 1;
    }
    assert!(count > 0);
}

fn catalog(mips: &[TextureMip], bytes: &[usize]) -> FileCatalog {
    let files: Vec<_> = mips
        .iter()
        .zip(bytes)
        .map(|(mip, bytes)| FileReceipt {
            file: mip.file.clone(),
            bytes: *bytes as u32,
            sha256: "0".repeat(64),
            image_size: Some([mip.width, mip.height]),
        })
        .collect();
    let mut catalog: FileCatalog = serde_json::from_value(serde_json::json!({
        "files": files, "plans": [], "dependency_staging_reserve": 1,
    }))
    .unwrap();
    catalog.admit().unwrap();
    catalog
}
#[test]
fn primary_admission_rejects_receipt_dimensions_and_nonhalving_plate_chain() {
    let mut mips = vec![
        TextureMip {
            file: "plate-base.png".into(),
            width: 16,
            height: 8,
        },
        TextureMip {
            file: "plate-tail.png".into(),
            width: 8,
            height: 4,
        },
    ];
    let receipts = catalog(&mips, &[128, 80]);
    assert!(png_chain_staging(&mips, 2, &receipts).is_ok());
    mips[1].width = 16;
    assert!(
        png_chain_staging(&mips, 2, &receipts)
            .unwrap_err()
            .to_string()
            .contains("receipt dimensions")
    );
    let nonhalving = catalog(&mips, &[128, 80]);
    assert!(
        png_chain_staging(&mips, 2, &nonhalving)
            .unwrap_err()
            .to_string()
            .contains("mip progression")
    );
    mips[1].width = 8;
    assert!(
        png_chain_staging(&mips, 1, &receipts)
            .unwrap_err()
            .to_string()
            .contains("mip count")
    );
}
