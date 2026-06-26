use serde::{Deserialize, Serialize};

pub const DATASET_SCOPE_FEATURE_V1: &str = "dataset_scope_v1";
pub const DATASET_POLICY_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatasetSelection {
    pub policy_version: u32,
    pub dataset_ids: Vec<String>,
}

impl DatasetSelection {
    pub fn new(policy_version: u32, dataset_ids: Vec<String>) -> Self {
        Self {
            policy_version,
            dataset_ids,
        }
    }
}
