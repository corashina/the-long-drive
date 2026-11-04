//! Validate runtime manifests with the same contract as the game.
//! `--encode` reads the exporter JSON and writes the validated manifest file to stdout.
use std::io::{self, Read, Write};
use the_long_slop::reference::{MAX_MANIFEST_BYTES, ReferenceManifest, encode_manifest};

fn validate() -> Result<(), Box<dyn std::error::Error>> {
    let encode = match std::env::args().nth(1).as_deref() {
        None => false,
        Some("--encode") => true,
        Some(other) => return Err(format!("Unknown argument: {other}").into()),
    };
    let mut content = Vec::new();
    io::stdin()
        .take(if encode {
            u64::MAX
        } else {
            MAX_MANIFEST_BYTES as u64 + 1
        })
        .read_to_end(&mut content)?;
    if encode {
        content = encode_manifest(&content)?;
    }
    ReferenceManifest::from_manifest(&content)?;
    if encode {
        io::stdout().lock().write_all(&content)?;
    }
    Ok(())
}

fn main() {
    if let Err(error) = validate() {
        eprintln!("Runtime manifest contract: {error}");
        std::process::exit(1);
    }
}
