//! Source `ls` word table: `ls.w` names, the English words of `ls.FillDefWW` and the installed language files.
//! The browser shows the words. The simulation checks word indices only.
use super::{ReferenceError, require};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LanguageReference {
    /// `ls.w` names in index order.
    pub names: Vec<String>,
    pub english: Vec<String>,
    pub languages: Vec<LanguageFile>,
}

/// One `TheLongDrive_Data/Languages` file. A null word shows the English word.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LanguageFile {
    pub name: String,
    pub words: Vec<Option<String>>,
}

impl LanguageReference {
    pub fn validate(&self) -> Result<(), ReferenceError> {
        require(
            !self.names.is_empty()
                && self.english.len() == self.names.len()
                && self
                    .languages
                    .iter()
                    .all(|l| !l.name.is_empty() && l.words.len() == self.names.len()),
            "language words",
        )
    }
}
