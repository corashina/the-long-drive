//! Enums with a `kind` tag in JSON and a binary (postcard) form.
//! Postcard cannot read internal tags, so the binary form tags externally.

/// Defines the enum once. `$name_text` is the enum name as a string.
macro_rules! tagged_enum {
    ($name_text:literal, $(#[$attr:meta])* $vis:vis enum $name:ident { $($body:tt)* }) => {
        #[derive(serde::Serialize, serde::Deserialize)]
        #[serde(remote = "Self", tag = "kind")]
        $(#[$attr])*
        $vis enum $name { $($body)* }
        const _: () = {
            #[allow(dead_code)]
            #[derive(serde::Serialize, serde::Deserialize)]
            #[serde(remote = $name_text)]
            $(#[$attr])*
            enum Binary { $($body)* }
            impl serde::Serialize for $name {
                fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                    if serializer.is_human_readable() {
                        $name::serialize(self, serializer)
                    } else {
                        Binary::serialize(self, serializer)
                    }
                }
            }
            impl<'de> serde::Deserialize<'de> for $name {
                fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                    if deserializer.is_human_readable() {
                        $name::deserialize(deserializer)
                    } else {
                        Binary::deserialize(deserializer)
                    }
                }
            }
        };
    };
}
