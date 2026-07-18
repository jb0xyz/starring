use authoring_promotion::{PromotionRecordV1, PromotionStageV1, PromotionStoreError};
use serde_json::Value;
use sqlx::types::Json;

pub(crate) const PROMOTION_COLUMNS: &str =
    "id, record_format_version, revision, stage, request_digest, tenant_id, principal_id, record";

#[derive(sqlx::FromRow)]
pub(crate) struct PromotionRow {
    pub id: String,
    pub record_format_version: i16,
    pub revision: i64,
    pub stage: String,
    pub request_digest: String,
    pub tenant_id: String,
    pub principal_id: String,
    pub record: Json<Value>,
}

pub(crate) fn backend(error: impl std::fmt::Display) -> PromotionStoreError {
    PromotionStoreError::Backend(error.to_string())
}

pub(crate) fn stage_name(stage: &PromotionStageV1) -> &'static str {
    match stage {
        PromotionStageV1::Prepared => "prepared",
        PromotionStageV1::Published { .. } => "published",
        PromotionStageV1::ActivationPending { .. } => "activation_pending",
        PromotionStageV1::Expired { .. } => "expired",
    }
}

pub(crate) fn decode_record(row: PromotionRow) -> Result<PromotionRecordV1, PromotionStoreError> {
    if row.record_format_version != 1 {
        return Err(backend("unsupported persisted promotion record format"));
    }
    let record = serde_json::from_value::<PromotionRecordV1>(row.record.0)
        .map_err(|error| backend(format!("invalid persisted promotion record: {error}")))?;
    record
        .validate()
        .map_err(PromotionStoreError::InvalidRecord)?;
    let revision = i64::try_from(record.revision.get())
        .map_err(|_| backend("persisted promotion revision exceeds BIGINT"))?;
    if row.id != record.id.as_str()
        || row.revision != revision
        || row.stage != stage_name(&record.stage)
        || row.request_digest != record.request_digest.as_str()
        || row.tenant_id != record.intent.authority.tenant_id.as_str()
        || row.principal_id != record.intent.authority.principal_id.as_str()
    {
        return Err(backend(
            "persisted promotion projections do not match record",
        ));
    }
    Ok(record)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_names_are_stable() {
        assert_eq!(stage_name(&PromotionStageV1::Prepared), "prepared");
    }
}
