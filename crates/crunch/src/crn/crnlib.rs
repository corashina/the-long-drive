// Supported compressed pixel formats.
// Basically all the standard DX9 formats, with some swizzled DXT5 formats
// (most of them supported by ATI's Compressonator), along with some ATI/X360 GPU specific formats.
#[derive(PartialEq, PartialOrd)]
#[repr(u32)]
pub enum CrnFormat {
    Invalid = 4294967295, // u32 -1,

    Dxt1 = 0,

    // cCRNFmtFirstValid = crn_format::cCRNFmtDXT1 as isize, // Rust doesn't allow same value enums, and as far as I see this is not used in the lib.

    // cCRNFmtDXT3 is not currently supported when writing to CRN - only DDS.
    Dxt3,

    CCrnfmtDxt5,

    // Various DXT5 derivatives
    Dxt5CcxY, // Luma-chroma
    Dxt5XGxR, // Swizzled 2-component
    Dxt5XGbr, // Swizzled 3-component
    Dxt5Agbr, // Swizzled 4-component

    // ATI 3DC and X360 DXN
    DxnXy,
    DxnYx,

    // DXT5 alpha blocks only
    Dxt5a,

    Etc1,
    Etc2,
    Etc2a,
    Etc1s,
    Etc2as,

    Total,
}
