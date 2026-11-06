use super::{tables_bytes, unpack};
use sha2::{Digest, Sha256};

/// Test files from texture2ddecoder. The hashes are the C++ unpacker output of all levels,
/// level 0, and levels 1+ (Python texture2ddecoder 1.0.6, as the exporter uses).
const FILES: [(&[u8], [&str; 3]); 2] = [
    (
        include_bytes!("../tests/UNITYCRUNCH_DXT1.crn"),
        [
            "3051e709ad93467df6fab6b7b090e475b8a1b789cbaf625a701a4376cfb383ed",
            "5ceb018722bc3c794a991091fb7f3a9434cad03ca2e24730aee8ffd65806d29f",
            "23d38faa575d4cf60e679273b401113506a35ff2d5e9e8981c7833760fb434c2",
        ],
    ),
    (
        include_bytes!("../tests/UNITYCRUNCH_DXT5.crn"),
        [
            "12efc390bbc2a9a23dc0881a6137078834240ec151aa4ad6d950f76a5ad3fb6e",
            "a4642e968bd4d2faeddd613a8052d89a6c68e3fab5460f5a2312282d1f0f9109",
            "0c397855c17c2d93174d14f3268972bbe32cfcc412122e3559e3d53d40836e1f",
        ],
    ),
];

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[test]
fn split_files_unpack_like_the_exporter() {
    for (file, [all, top, rest]) in FILES {
        let tables = tables_bytes(file).unwrap();
        let level1 = u32::from_be_bytes(file[74..78].try_into().unwrap()) as usize;
        assert_eq!(hash(&unpack(file, 0).unwrap()), all);
        let top_file = [&file[..level1]].concat();
        assert_eq!(hash(&unpack(&top_file, 0).unwrap()), top);
        let start = [&file[..tables], &file[level1..]].concat();
        assert_eq!(hash(&unpack(&start, 1).unwrap()), rest);
        assert!(unpack(&start[..start.len() - 1], 1).is_err());
    }
}

#[test]
fn bit_reader_reads_zero_past_its_end() {
    use crate::crn::crn_symbol_codec::symbol_codec;
    let data = [0xab, 0xcd, 0xef];
    let mut codec = symbol_codec::default();
    // The reader gets only the first byte, so it must not read on into the rest of the slice.
    assert!(codec.start_decoding(&data, 1));
    assert_eq!(codec.decode_bits(8), Ok(0xab));
    assert_eq!(codec.decode_bits(24), Ok(0));
    assert!(!codec.start_decoding(&data, 4));
}
