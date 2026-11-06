//! Terrain job input identity. A tile keeps the context revision and the hash of its input.
//! A check at the same revision does not encode the input again.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(in crate::world) struct TileInput {
    /// Terrain context revision of the last encode or check.
    pub revision: u64,
    /// Hash of the encoded job input. Equal inputs build equal tiles.
    pub hash: u64,
}

impl TileInput {
    pub(in crate::world) fn new(revision: u64, encoded: &[u8]) -> Self {
        Self {
            revision,
            hash: hash(encoded),
        }
    }

    /// True if the input at `revision` is the same. `encode` runs only for an older revision.
    /// It writes the current input to its argument.
    pub(in crate::world) fn current<E>(
        &mut self,
        revision: u64,
        encoded: &mut Vec<u8>,
        encode: impl FnOnce(&mut Vec<u8>) -> Result<(), E>,
    ) -> Result<bool, E> {
        if self.revision == revision {
            return Ok(true);
        }
        encode(encoded)?;
        let same = hash(encoded) == self.hash;
        if same {
            self.revision = revision;
        }
        Ok(same)
    }
}

fn hash(input: &[u8]) -> u64 {
    use std::hash::Hasher;
    let mut hasher = std::hash::DefaultHasher::new();
    hasher.write(input);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::TileInput;

    #[test]
    fn an_unchanged_input_does_not_encode_again() {
        let mut tile = TileInput::new(1, b"tile");
        let mut encoded = Vec::new();
        let mut encodes = 0;
        let mut check = |tile: &mut TileInput, revision, input: &'static [u8]| {
            tile.current(revision, &mut encoded, |out| {
                encodes += 1;
                out.clear();
                out.extend_from_slice(input);
                Ok::<_, ()>(())
            })
            .unwrap()
        };
        assert!(check(&mut tile, 1, b"tile"));
        assert!(check(&mut tile, 2, b"tile"));
        assert!(
            check(&mut tile, 2, b"other"),
            "revision 2 is already checked"
        );
        assert!(!check(&mut tile, 3, b"other"));
        drop(check);
        assert_eq!(encodes, 2);
    }
}
