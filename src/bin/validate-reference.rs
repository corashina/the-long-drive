//! Validate runtime manifests with the same contract as the game.
//! `validate-reference <file>` checks a gzip manifest file. The build checks the file it ships.
//! `--encode` reads the exporter JSON and writes the validated manifest file to stdout.
//! `--crunch` reads stored crunch textures and checks that the game unpacker gives the exporter's BC blocks.
use sha2::{Digest, Sha256};
use std::io::{self, Read, Write};
use thelongdrive::reference::{ReferenceManifest, encode_manifest, read_manifest};

fn validate_file(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let start = std::time::Instant::now();
    ReferenceManifest::from_manifest(&read_manifest(path)?)?;
    eprintln!(
        "Runtime manifest contract: {path} passed in {:.2?}",
        start.elapsed()
    );
    Ok(())
}

fn encode() -> Result<(), Box<dyn std::error::Error>> {
    let mut content = Vec::new();
    io::stdin().read_to_end(&mut content)?;
    let content = encode_manifest(&content)?;
    ReferenceManifest::from_manifest(&content)?;
    io::stdout().lock().write_all(&content)?;
    Ok(())
}

/// Input: for each texture, the start file and the top file (each a little-endian u32 length
/// and the bytes, length 0 for no top file), then the SHA-256 of the BC blocks of all levels.
/// The check unpacks the files as the game does with a late top mip.
fn check_crunch() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = Vec::new();
    io::stdin().read_to_end(&mut input)?;
    let mut rest = &input[..];
    let mut take = |count: usize| -> Result<&[u8], String> {
        let (head, tail) = rest
            .split_at_checked(count)
            .ok_or("Truncated crunch check input")?;
        rest = tail;
        Ok(head)
    };
    let mut index = 0;
    loop {
        let Ok(length) = take(4) else { break };
        let start = take(u32::from_le_bytes(length.try_into()?) as usize)?.to_vec();
        let length = u32::from_le_bytes(take(4)?.try_into()?) as usize;
        let top = take(length)?.to_vec();
        let expected = take(32)?;
        // A malformed texture fails the check with its index, never a bare panic.
        let blocks = std::panic::catch_unwind(|| -> Result<Vec<u8>, String> {
            if top.is_empty() {
                return crunch::unpack(&start, 0);
            }
            let tables = crunch::tables_bytes(&start)?;
            let mut blocks = crunch::unpack(&[&start[..tables], &top].concat(), 0)?;
            blocks.extend(crunch::unpack(&start, 1)?);
            Ok(blocks)
        })
        .unwrap_or_else(|_| Err("unpacker panic".into()))
        .map_err(|e| format!("Crunch texture {index}: {e}"))?;
        if Sha256::digest(&blocks)[..] != *expected {
            return Err(
                format!("Crunch texture {index}: BC blocks differ from the exporter").into(),
            );
        }
        index += 1;
    }
    Ok(())
}

fn main() {
    let result = match std::env::args().nth(1).as_deref() {
        Some("--encode") => encode(),
        Some("--crunch") => check_crunch(),
        Some(path) if !path.starts_with('-') => validate_file(path),
        _ => Err("Usage: validate-reference <manifest.bin.gz> | --encode | --crunch".into()),
    };
    if let Err(error) = result {
        eprintln!("Runtime manifest contract: {error}");
        std::process::exit(1);
    }
}
