//! Unity crunch unpacker from texture2ddecoder 0.1.2 (MIT OR Apache-2.0, see the license files).
//! Changes: only the Unity crunch modules, module paths, no texture info or RGBA decode,
//! and wrapping pitch steps (upstream relies on release overflow).
#![allow(clippy::all, dead_code, non_camel_case_types, unused_assignments)]
pub(crate) mod crn_consts;
pub(crate) mod crn_header;
pub(crate) mod crn_static_huffman_data_model;
pub(crate) mod crn_symbol_codec;
pub(crate) mod crn_unpacker;
pub(crate) mod crn_utils;
pub(crate) mod crnlib;
#[macro_use]
pub(crate) mod macros;
