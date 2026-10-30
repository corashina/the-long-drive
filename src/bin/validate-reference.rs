//! Validate runtime manifests with the same contract as the game.
use std::io::{self, Read};
use the_long_slop::reference::{AudioReference, MAX_MANIFEST_BYTES, ReferenceManifest};

fn validate() -> Result<(), Box<dyn std::error::Error>> {
    let mut content = Vec::new();
    io::stdin()
        .take(MAX_MANIFEST_BYTES as u64 + 1)
        .read_to_end(&mut content)?;
    let game = ReferenceManifest::from_json(&content)?;
    AudioReference::from_value(&game.audio, &game)?;
    Ok(())
}

fn main() {
    if let Err(error) = validate() {
        eprintln!("Runtime manifest contract: {error}");
        std::process::exit(1);
    }
}
