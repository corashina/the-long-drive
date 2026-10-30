//! Final runtime file receipts. Admission builds these path indices once.
use super::{ReferenceError, require};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileReceipt {
    pub file: String,
    pub bytes: u32,
    pub sha256: String,
    pub image_size: Option<[u32; 2]>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanReceipt {
    pub asset_file: String,
    pub input: FileReceipt,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileCatalog {
    pub files: Vec<FileReceipt>,
    pub plans: Vec<PlanReceipt>,
    pub dependency_staging_reserve: u32,
    #[serde(skip)]
    paths: HashMap<String, usize>,
    #[serde(skip)]
    plan_paths: HashMap<String, usize>,
}

impl FileReceipt {
    pub fn validate(&self) -> Result<(), ReferenceError> {
        require(
            !self.file.is_empty()
                && self.file.split('/').all(|part| {
                    !part.is_empty()
                        && part != "."
                        && part != ".."
                        && !part.starts_with(' ')
                        && !part.ends_with(' ')
                        && part
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"_.- ".contains(&b))
                }),
            "package file path",
        )?;
        require(
            self.sha256.len() == 64
                && self
                    .sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "package file receipt",
        )?;
        if self.file.ends_with(".png") {
            let Some([width, height]) = self.image_size else {
                return Err(ReferenceError("Missing PNG receipt dimensions".into()));
            };
            require(
                width > 0
                    && height > 0
                    && width
                        .checked_mul(height)
                        .and_then(|v| v.checked_mul(4))
                        .is_some(),
                "PNG receipt dimensions",
            )?;
        } else {
            require(self.image_size.is_none(), "non-PNG image receipt")?;
        }
        Ok(())
    }
    fn retained_bytes(&self) -> usize {
        self.file.capacity() + self.sha256.capacity()
    }
}

impl FileCatalog {
    pub fn admit(&mut self) -> Result<(), ReferenceError> {
        require(
            self.dependency_staging_reserve > 0
                && self.dependency_staging_reserve < 32 * 1024 * 1024,
            "dependency staging reserve",
        )?;
        self.paths = HashMap::with_capacity(self.files.len());
        for (index, receipt) in self.files.iter().enumerate() {
            receipt.validate()?;
            require(
                self.paths.insert(receipt.file.clone(), index).is_none(),
                "duplicate file receipt",
            )?;
        }
        self.plan_paths = HashMap::with_capacity(self.plans.len());
        for (index, plan) in self.plans.iter().enumerate() {
            require(
                plan.asset_file.ends_with(".gltf") && self.paths.contains_key(&plan.asset_file),
                "plan asset receipt",
            )?;
            require(
                self.file(&plan.input.file)? == &plan.input,
                "plan file receipt",
            )?;
            require(
                self.plan_paths
                    .insert(plan.asset_file.clone(), index)
                    .is_none(),
                "duplicate plan receipt",
            )?;
        }
        Ok(())
    }
    pub fn file(&self, path: &str) -> Result<&FileReceipt, ReferenceError> {
        self.paths
            .get(path)
            .map(|&index| &self.files[index])
            .ok_or_else(|| ReferenceError(format!("Missing file receipt: {path}")))
    }
    pub fn plan(&self, asset: &str) -> Result<&FileReceipt, ReferenceError> {
        self.plan_paths
            .get(asset)
            .map(|&index| &self.plans[index].input)
            .ok_or_else(|| ReferenceError(format!("Missing plan receipt: {asset}")))
    }
    pub fn check_plan_reservation(
        &self,
        asset: &str,
        cpu: usize,
        staging: usize,
    ) -> Result<(), ReferenceError> {
        let bytes = self.plan(asset)?.bytes as usize;
        require(
            bytes
                .checked_mul(18)
                .is_some_and(|required| required <= cpu)
                && bytes
                    .checked_mul(2)
                    .is_some_and(|required| required <= staging),
            "plan reservation",
        )
    }
    pub fn retained_bytes(&self) -> usize {
        use std::mem::size_of;
        size_of::<Self>()
            + self.files.capacity() * size_of::<FileReceipt>()
            + self.plans.capacity() * size_of::<PlanReceipt>()
            + self
                .files
                .iter()
                .map(FileReceipt::retained_bytes)
                .sum::<usize>()
            + self
                .plans
                .iter()
                .map(|p| p.asset_file.capacity() + p.input.retained_bytes())
                .sum::<usize>()
            + [&self.paths, &self.plan_paths]
                .iter()
                .map(|map| {
                    map.capacity() * (size_of::<(String, usize)>() + 16)
                        + map.keys().map(String::capacity).sum::<usize>()
                })
                .sum::<usize>()
    }
}
