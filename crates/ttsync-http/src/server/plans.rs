use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

use ttsync_contract::path::SyncPath;
use ttsync_contract::peer::DeviceId;
use ttsync_contract::plan::{PlanId, SyncPlan};
use ttsync_contract::sync::SyncMode;
use ttsync_core::error::SyncError;

const PLAN_TTL_MS: u64 = 30 * 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PlanDirection {
    Pull,
    Push,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct TransferMeta {
    pub size_bytes: u64,
    pub modified_ms: u64,
}

#[derive(Debug)]
pub(super) struct PlanRecord {
    pub direction: PlanDirection,
    pub device_id: DeviceId,
    pub mode: SyncMode,
    pub transfer: HashMap<SyncPath, TransferMeta>,
    pub delete: Vec<SyncPath>,
    created_at_ms: u64,
}

#[derive(Debug, Clone)]
pub(super) struct PlanSnapshot {
    pub direction: PlanDirection,
    pub transfer: HashMap<SyncPath, TransferMeta>,
}

#[derive(Debug, Default)]
pub(super) struct PlanStore {
    records: Mutex<HashMap<String, PlanRecord>>,
}

impl PlanStore {
    pub fn insert(
        &self,
        device_id: DeviceId,
        direction: PlanDirection,
        mode: SyncMode,
        plan: SyncPlan,
    ) -> Result<(), SyncError> {
        let now_ms = now_ms()?;
        let mut records = self.records()?;
        retain_unexpired(&mut records, now_ms);

        let SyncPlan {
            plan_id: PlanId(plan_id),
            transfer,
            delete,
            selection: _,
            files_total: _,
            bytes_total: _,
        } = plan;

        let transfer = transfer
            .into_iter()
            .map(|entry| {
                (
                    entry.path,
                    TransferMeta {
                        size_bytes: entry.size_bytes,
                        modified_ms: entry.modified_ms,
                    },
                )
            })
            .collect::<HashMap<_, _>>();

        records.insert(
            plan_id,
            PlanRecord {
                direction,
                device_id,
                mode,
                transfer,
                delete,
                created_at_ms: now_ms,
            },
        );

        Ok(())
    }

    pub fn snapshot(&self, plan_id: &str, device_id: &DeviceId) -> Result<PlanSnapshot, SyncError> {
        let now_ms = now_ms()?;
        let mut records = self.records()?;
        retain_unexpired(&mut records, now_ms);

        let record = records
            .get(plan_id)
            .ok_or_else(|| SyncError::NotFound("plan not found".into()))?;

        ensure_plan_owner(record, device_id)?;

        Ok(PlanSnapshot {
            direction: record.direction,
            transfer: record.transfer.clone(),
        })
    }

    pub fn transfer_meta(
        &self,
        plan_id: &str,
        device_id: &DeviceId,
        sync_path: &SyncPath,
    ) -> Result<(PlanDirection, TransferMeta), SyncError> {
        let now_ms = now_ms()?;
        let mut records = self.records()?;
        retain_unexpired(&mut records, now_ms);

        let record = records
            .get(plan_id)
            .ok_or_else(|| SyncError::NotFound("plan not found".into()))?;

        ensure_plan_owner(record, device_id)?;

        let meta = record
            .transfer
            .get(sync_path)
            .ok_or_else(|| SyncError::NotFound("file not in plan".into()))?;

        Ok((record.direction, *meta))
    }

    pub fn take_if<F>(
        &self,
        plan_id: &str,
        device_id: &DeviceId,
        validate: F,
    ) -> Result<PlanRecord, SyncError>
    where
        F: FnOnce(&PlanRecord) -> Result<(), SyncError>,
    {
        let now_ms = now_ms()?;
        let mut records = self.records()?;
        retain_unexpired(&mut records, now_ms);

        let record = records
            .get(plan_id)
            .ok_or_else(|| SyncError::NotFound("plan not found".into()))?;
        ensure_plan_owner(record, device_id)?;
        validate(record)?;

        records
            .remove(plan_id)
            .ok_or_else(|| SyncError::Internal("plan disappeared while locked".into()))
    }

    fn records(&self) -> Result<MutexGuard<'_, HashMap<String, PlanRecord>>, SyncError> {
        self.records
            .lock()
            .map_err(|_| SyncError::Internal("plans mutex poisoned".into()))
    }
}

fn retain_unexpired(records: &mut HashMap<String, PlanRecord>, now_ms: u64) {
    records.retain(|_, record| record.created_at_ms + PLAN_TTL_MS > now_ms);
}

fn ensure_plan_owner(record: &PlanRecord, device_id: &DeviceId) -> Result<(), SyncError> {
    if &record.device_id != device_id {
        return Err(SyncError::Unauthorized(
            "plan does not belong to this peer".into(),
        ));
    }

    Ok(())
}

fn now_ms() -> Result<u64, SyncError> {
    let duration = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| SyncError::Internal(e.to_string()))?;
    Ok(duration.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    use ttsync_contract::dataset::{DATASET_POLICY_VERSION, DatasetSelection};
    use ttsync_contract::manifest::ManifestEntryV2;

    fn device_id(value: &str) -> DeviceId {
        DeviceId::new(value.to_owned()).expect("valid device id")
    }

    fn plan(plan_id: &str, path: &str) -> SyncPlan {
        SyncPlan {
            plan_id: PlanId(plan_id.to_owned()),
            selection: DatasetSelection::new(
                DATASET_POLICY_VERSION,
                vec!["chat.character.history".to_owned()],
            ),
            transfer: vec![ManifestEntryV2 {
                path: SyncPath::new(path.to_owned()).expect("valid sync path"),
                size_bytes: 4,
                modified_ms: 1000,
                content_hash: None,
            }],
            delete: vec![],
            files_total: 1,
            bytes_total: 4,
        }
    }

    #[test]
    fn failed_validator_does_not_consume_plan() {
        let store = PlanStore::default();
        let owner = device_id("00000000-0000-4000-8000-000000000001");
        store
            .insert(
                owner.clone(),
                PlanDirection::Push,
                SyncMode::Mirror,
                plan("plan-a", "default-user/chats/a.jsonl"),
            )
            .expect("insert plan");

        let denied = store.take_if("plan-a", &owner, |_| {
            Err(SyncError::Unauthorized("mirror delete not granted".into()))
        });
        assert!(matches!(denied, Err(SyncError::Unauthorized(_))));

        let record = store
            .take_if("plan-a", &owner, |_| Ok(()))
            .expect("plan should remain after failed validation");
        assert_eq!(record.mode, SyncMode::Mirror);
    }

    #[test]
    fn wrong_owner_does_not_consume_plan() {
        let store = PlanStore::default();
        let owner = device_id("00000000-0000-4000-8000-000000000001");
        let other = device_id("00000000-0000-4000-8000-000000000002");
        store
            .insert(
                owner.clone(),
                PlanDirection::Push,
                SyncMode::Incremental,
                plan("plan-b", "default-user/chats/b.jsonl"),
            )
            .expect("insert plan");

        let denied = store.take_if("plan-b", &other, |_| Ok(()));
        assert!(matches!(denied, Err(SyncError::Unauthorized(_))));

        store
            .take_if("plan-b", &owner, |_| Ok(()))
            .expect("owner should still be able to consume plan");
    }
}
