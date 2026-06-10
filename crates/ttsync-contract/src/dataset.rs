use serde::{Deserialize, Serialize};

pub const DATASET_SCOPE_FEATURE_V1: &str = "dataset_scope_v1";
pub const DATASET_POLICY_VERSION: u32 = 1;
pub const LEGACY_V2_DATASET_ID: &str = "legacy.v2";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatasetSelection {
    #[serde(default = "default_policy_version")]
    pub policy_version: u32,
    #[serde(default = "legacy_dataset_ids")]
    pub dataset_ids: Vec<String>,
}

impl DatasetSelection {
    pub fn new(policy_version: u32, dataset_ids: Vec<String>) -> Self {
        Self {
            policy_version,
            dataset_ids,
        }
    }

    pub fn legacy_v2() -> Self {
        Self {
            policy_version: DATASET_POLICY_VERSION,
            dataset_ids: legacy_dataset_ids(),
        }
    }
}

impl Default for DatasetSelection {
    fn default() -> Self {
        Self::legacy_v2()
    }
}

fn default_policy_version() -> u32 {
    DATASET_POLICY_VERSION
}

fn legacy_dataset_ids() -> Vec<String> {
    vec![LEGACY_V2_DATASET_ID.to_owned()]
}
