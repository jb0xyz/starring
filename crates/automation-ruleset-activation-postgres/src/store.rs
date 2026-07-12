use automation_ruleset_activation::{
    ActivationRequest, ActivationRequestId, ActivationRequestStore, ActivationStoreError,
    ApplyAttemptId, ApplyErrorRecord, ApprovalDecisionError, ApproveError, ClaimDecision,
    ClaimOutcome, CompletionKind, CreateActivationRequest, RejectError, RejectionDecisionError,
};
use chrono::{DateTime, Utc};
use discord_model::{GuildId, UserId};
use sqlx::postgres::PgPool;
use sqlx::types::Json;
use sqlx::{PgConnection, Row};

use crate::row::{
    backend, completion_kind_str, decode_request, state_str, ActivationRequestRow, ApprovalRow,
    REQUEST_COLUMNS,
};

const APPLYING_CONSTRAINT: &str = "activation_requests_one_applying_per_ruleset";

pub struct PostgresActivationRequestStore {
    pool: PgPool,
}

impl PostgresActivationRequestStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

async fn fetch_request(
    connection: &mut PgConnection,
    request_id: &ActivationRequestId,
    for_update: bool,
) -> Result<Option<ActivationRequest>, ActivationStoreError> {
    let lock = if for_update { " FOR UPDATE" } else { "" };
    let row = sqlx::query_as::<_, ActivationRequestRow>(&format!(
        "SELECT {REQUEST_COLUMNS} FROM activation_requests WHERE id = $1{lock}"
    ))
    .bind(request_id.as_str())
    .fetch_optional(&mut *connection)
    .await
    .map_err(backend)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let approvals = sqlx::query_as::<_, ApprovalRow>(
        "SELECT approver_id, approved_at FROM activation_request_approvals \
         WHERE request_id = $1 ORDER BY approver_id",
    )
    .bind(request_id.as_str())
    .fetch_all(&mut *connection)
    .await
    .map_err(backend)?;
    decode_request(row, approvals).map(Some)
}

async fn database_now(
    connection: &mut PgConnection,
) -> Result<DateTime<Utc>, ActivationStoreError> {
    sqlx::query_scalar("SELECT NOW()")
        .fetch_one(connection)
        .await
        .map_err(backend)
}

async fn database_lease(
    connection: &mut PgConnection,
    lease_seconds: i64,
) -> Result<(DateTime<Utc>, DateTime<Utc>), ActivationStoreError> {
    if lease_seconds <= 0 {
        return Err(ActivationStoreError::InvalidRequest(
            "lease duration must be positive".to_string(),
        ));
    }
    sqlx::query_as("SELECT NOW(), NOW() + ($1 * INTERVAL '1 second')")
        .bind(lease_seconds)
        .fetch_one(connection)
        .await
        .map_err(backend)
}

fn is_constraint(error: &sqlx::Error, name: &str) -> bool {
    matches!(error, sqlx::Error::Database(database) if database.constraint() == Some(name))
}

fn decision_outcome(decision: ClaimDecision, request: ActivationRequest) -> ClaimOutcome {
    match decision {
        ClaimDecision::Claimed => ClaimOutcome::Claimed(Box::new(request)),
        ClaimDecision::InProgress {
            blocking_request_id,
            lease_until,
            lease_expired,
        } => ClaimOutcome::InProgress {
            blocking_request_id,
            lease_until,
            lease_expired,
        },
        ClaimDecision::AlreadyApplied => ClaimOutcome::AlreadyApplied,
        ClaimDecision::NotApproved => ClaimOutcome::NotApproved,
        ClaimDecision::Expired => ClaimOutcome::Expired,
    }
}

async fn blocking_request(
    pool: &PgPool,
    request: &ActivationRequest,
) -> Result<ClaimOutcome, ActivationStoreError> {
    let row = sqlx::query(
        "SELECT id, apply_lease_until, apply_lease_until <= NOW() AS lease_expired \
         FROM activation_requests \
         WHERE guild_id = $1 AND ruleset_key = $2 AND state = 'applying' \
         ORDER BY id LIMIT 1",
    )
    .bind(request.target.guild_id.to_string())
    .bind(request.target.ruleset_key.as_str())
    .fetch_optional(pool)
    .await
    .map_err(backend)?
    .ok_or_else(|| backend("applying constraint conflict without blocking row"))?;
    let id: String = row.try_get("id").map_err(backend)?;
    let lease_until: DateTime<Utc> = row.try_get("apply_lease_until").map_err(backend)?;
    let lease_expired: bool = row.try_get("lease_expired").map_err(backend)?;
    Ok(ClaimOutcome::InProgress {
        blocking_request_id: ActivationRequestId::parse(&id)
            .map_err(|error| backend(format!("invalid blocking request id: {error}")))?,
        lease_until,
        lease_expired,
    })
}

impl ActivationRequestStore for PostgresActivationRequestStore {
    async fn create(
        &self,
        input: CreateActivationRequest,
    ) -> Result<ActivationRequest, ActivationStoreError> {
        let _ = ActivationRequest::create(input.clone(), DateTime::<Utc>::UNIX_EPOCH)
            .map_err(ActivationStoreError::InvalidRequest)?;
        let ttl_millis = input.ttl.num_milliseconds();
        if ttl_millis <= 0 {
            return Err(ActivationStoreError::InvalidRequest(
                "request ttl must be positive".to_string(),
            ));
        }
        let observed_version = input
            .observed_active
            .as_ref()
            .map(|observed| i64::from(observed.version.get()));
        let observed_hash = input
            .observed_active
            .as_ref()
            .map(|observed| observed.content_hash.to_hex());
        let mut tx = self.pool.begin().await.map_err(backend)?;
        let result = sqlx::query(
            "INSERT INTO activation_requests (id, guild_id, ruleset_key, target_version, \
             target_content_hash, requester_id, required_approvals, state, created_at, expires_at, \
             observed_active_version, observed_active_hash) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, 'pending', NOW(), \
             NOW() + ($8 * INTERVAL '1 millisecond'), $9, $10)",
        )
        .bind(input.id.as_str())
        .bind(input.target.guild_id.to_string())
        .bind(input.target.ruleset_key.as_str())
        .bind(i64::from(input.target.version.get()))
        .bind(input.target.content_hash.to_hex())
        .bind(input.requester.to_string())
        .bind(i32::try_from(input.required_approvals).map_err(|_| {
            ActivationStoreError::InvalidRequest("required approvals overflow".to_string())
        })?)
        .bind(ttl_millis)
        .bind(observed_version)
        .bind(observed_hash)
        .execute(&mut *tx)
        .await;
        if let Err(error) = result {
            tx.rollback().await.map_err(backend)?;
            if is_constraint(&error, "activation_requests_pkey") {
                return Err(ActivationStoreError::DuplicateRequest);
            }
            return Err(backend(error));
        }
        let request = fetch_request(&mut tx, &input.id, false)
            .await?
            .ok_or(ActivationStoreError::NotFound)?;
        tx.commit().await.map_err(backend)?;
        Ok(request)
    }

    async fn get(
        &self,
        request_id: &ActivationRequestId,
    ) -> Result<Option<ActivationRequest>, ActivationStoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        sqlx::query(
            "UPDATE activation_requests SET state = 'expired' \
             WHERE id = $1 AND state IN ('pending','approved') AND expires_at <= NOW()",
        )
        .bind(request_id.as_str())
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        let request = fetch_request(&mut tx, request_id, false).await?;
        tx.commit().await.map_err(backend)?;
        Ok(request)
    }

    async fn approve(
        &self,
        request_id: &ActivationRequestId,
        approver: UserId,
    ) -> Result<ActivationRequest, ApproveError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        let mut request = fetch_request(&mut tx, request_id, true)
            .await?
            .ok_or(ActivationStoreError::NotFound)?;
        let now = database_now(&mut tx).await?;
        if let Err(error) = request.approve_at(approver, now) {
            if error == ApprovalDecisionError::Expired {
                sqlx::query("UPDATE activation_requests SET state = 'expired' WHERE id = $1")
                    .bind(request_id.as_str())
                    .execute(&mut *tx)
                    .await
                    .map_err(backend)?;
                tx.commit().await.map_err(backend)?;
            } else {
                tx.rollback().await.map_err(backend)?;
            }
            return Err(error.into());
        }
        let approval = request
            .approvals
            .iter()
            .find(|approval| approval.approver == approver)
            .ok_or_else(|| backend("approval decision omitted approver"))?;
        let insert = sqlx::query(
            "INSERT INTO activation_request_approvals (request_id, approver_id, approved_at) \
             VALUES ($1, $2, $3)",
        )
        .bind(request_id.as_str())
        .bind(approver.to_string())
        .bind(approval.approved_at)
        .execute(&mut *tx)
        .await;
        if let Err(error) = insert {
            tx.rollback().await.map_err(backend)?;
            if is_constraint(&error, "activation_request_approvals_pkey") {
                return Err(ApproveError::DuplicateApproval);
            }
            return Err(ApproveError::Store(backend(error)));
        }
        sqlx::query("UPDATE activation_requests SET state = $2 WHERE id = $1")
            .bind(request_id.as_str())
            .bind(state_str(request.state))
            .execute(&mut *tx)
            .await
            .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(request)
    }

    async fn reject(
        &self,
        request_id: &ActivationRequestId,
        rejected_by: UserId,
        reason: String,
    ) -> Result<ActivationRequest, RejectError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        let mut request = fetch_request(&mut tx, request_id, true)
            .await?
            .ok_or(ActivationStoreError::NotFound)?;
        let now = database_now(&mut tx).await?;
        if let Err(error) = request.reject_at(rejected_by, reason, now) {
            if error == RejectionDecisionError::Expired {
                sqlx::query("UPDATE activation_requests SET state = 'expired' WHERE id = $1")
                    .bind(request_id.as_str())
                    .execute(&mut *tx)
                    .await
                    .map_err(backend)?;
                tx.commit().await.map_err(backend)?;
            } else {
                tx.rollback().await.map_err(backend)?;
            }
            return Err(error.into());
        }
        let rejection = request
            .rejection
            .as_ref()
            .ok_or_else(|| backend("rejection decision omitted rejection"))?;
        sqlx::query(
            "UPDATE activation_requests SET state = 'rejected', rejected_at = $2, \
             rejected_by = $3, rejection_reason = $4 WHERE id = $1 AND state = 'pending'",
        )
        .bind(request_id.as_str())
        .bind(rejection.rejected_at)
        .bind(rejection.rejected_by.to_string())
        .bind(&rejection.reason)
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(request)
    }

    async fn claim_apply(
        &self,
        request_id: &ActivationRequestId,
        attempt_id: ApplyAttemptId,
        lease_seconds: i64,
    ) -> Result<ClaimOutcome, ActivationStoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        let mut request = fetch_request(&mut tx, request_id, true)
            .await?
            .ok_or(ActivationStoreError::NotFound)?;
        let (now, lease_until) = database_lease(&mut tx, lease_seconds).await?;
        let decision = request
            .claim_apply_at(attempt_id.clone(), now, lease_until)
            .map_err(|error| ActivationStoreError::InvalidRequest(error.to_string()))?;
        if decision == ClaimDecision::Expired {
            sqlx::query("UPDATE activation_requests SET state = 'expired' WHERE id = $1")
                .bind(request_id.as_str())
                .execute(&mut *tx)
                .await
                .map_err(backend)?;
            tx.commit().await.map_err(backend)?;
            return Ok(ClaimOutcome::Expired);
        }
        if decision != ClaimDecision::Claimed {
            tx.commit().await.map_err(backend)?;
            return Ok(decision_outcome(decision, request));
        }
        let update = sqlx::query(
            "UPDATE activation_requests SET state = 'applying', apply_attempt_id = $2, \
             apply_attempt_no = apply_attempt_no + 1, \
             apply_lease_until = NOW() + ($3 * INTERVAL '1 second') \
             WHERE id = $1 AND state = 'approved' AND expires_at > NOW()",
        )
        .bind(request_id.as_str())
        .bind(attempt_id.as_str())
        .bind(lease_seconds)
        .execute(&mut *tx)
        .await;
        match update {
            Ok(result) if result.rows_affected() == 1 => {
                let claimed = fetch_request(&mut tx, request_id, false)
                    .await?
                    .ok_or(ActivationStoreError::NotFound)?;
                tx.commit().await.map_err(backend)?;
                Ok(ClaimOutcome::Claimed(Box::new(claimed)))
            }
            Ok(_) => {
                tx.rollback().await.map_err(backend)?;
                Err(backend("apply claim CAS failed"))
            }
            Err(error) if is_constraint(&error, APPLYING_CONSTRAINT) => {
                tx.rollback().await.map_err(backend)?;
                blocking_request(&self.pool, &request).await
            }
            Err(error) => {
                tx.rollback().await.map_err(backend)?;
                Err(backend(error))
            }
        }
    }

    async fn claim_resume(
        &self,
        request_id: &ActivationRequestId,
        attempt_id: ApplyAttemptId,
        lease_seconds: i64,
    ) -> Result<ClaimOutcome, ActivationStoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        let mut request = fetch_request(&mut tx, request_id, true)
            .await?
            .ok_or(ActivationStoreError::NotFound)?;
        let (now, lease_until) = database_lease(&mut tx, lease_seconds).await?;
        let decision = request
            .claim_resume_at(attempt_id.clone(), now, lease_until)
            .map_err(|error| ActivationStoreError::InvalidRequest(error.to_string()))?;
        if decision != ClaimDecision::Claimed {
            tx.commit().await.map_err(backend)?;
            return Ok(decision_outcome(decision, request));
        }
        let result = sqlx::query(
            "UPDATE activation_requests SET apply_attempt_id = $2, \
             apply_attempt_no = apply_attempt_no + 1, \
             apply_lease_until = NOW() + ($3 * INTERVAL '1 second') \
             WHERE id = $1 AND state = 'applying' AND apply_lease_until <= NOW()",
        )
        .bind(request_id.as_str())
        .bind(attempt_id.as_str())
        .bind(lease_seconds)
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        if result.rows_affected() != 1 {
            tx.rollback().await.map_err(backend)?;
            return Err(backend("resume claim CAS failed"));
        }
        let claimed = fetch_request(&mut tx, request_id, false)
            .await?
            .ok_or(ActivationStoreError::NotFound)?;
        tx.commit().await.map_err(backend)?;
        Ok(ClaimOutcome::Claimed(Box::new(claimed)))
    }

    async fn renew_lease(
        &self,
        request_id: &ActivationRequestId,
        attempt_id: &ApplyAttemptId,
        lease_seconds: i64,
    ) -> Result<bool, ActivationStoreError> {
        if lease_seconds <= 0 {
            return Err(ActivationStoreError::InvalidRequest(
                "lease duration must be positive".to_string(),
            ));
        }
        let result = sqlx::query(
            "UPDATE activation_requests SET apply_lease_until = \
             NOW() + ($3 * INTERVAL '1 second') \
             WHERE id = $1 AND state = 'applying' AND apply_attempt_id = $2",
        )
        .bind(request_id.as_str())
        .bind(attempt_id.as_str())
        .bind(lease_seconds)
        .execute(&self.pool)
        .await
        .map_err(backend)?;
        Ok(result.rows_affected() == 1)
    }

    async fn complete_applied(
        &self,
        request_id: &ActivationRequestId,
        attempt_id: &ApplyAttemptId,
        applied_by: UserId,
        kind: CompletionKind,
        notices: Option<Vec<String>>,
    ) -> Result<bool, ActivationStoreError> {
        let result = sqlx::query(
            "UPDATE activation_requests SET state = 'applied', apply_attempt_id = NULL, \
             apply_lease_until = NULL, last_apply_error = NULL, applied_at = NOW(), \
             applied_by = $3, completion_kind = $4, activation_notices = $5 \
             WHERE id = $1 AND state = 'applying' AND apply_attempt_id = $2",
        )
        .bind(request_id.as_str())
        .bind(attempt_id.as_str())
        .bind(applied_by.to_string())
        .bind(completion_kind_str(kind))
        .bind(notices.map(Json))
        .execute(&self.pool)
        .await
        .map_err(backend)?;
        Ok(result.rows_affected() == 1)
    }

    async fn release_to_approved(
        &self,
        request_id: &ActivationRequestId,
        attempt_id: &ApplyAttemptId,
        error: ApplyErrorRecord,
    ) -> Result<bool, ActivationStoreError> {
        let result = sqlx::query(
            "UPDATE activation_requests SET state = 'approved', apply_attempt_id = NULL, \
             apply_lease_until = NULL, last_apply_error = $3 \
             WHERE id = $1 AND state = 'applying' AND apply_attempt_id = $2",
        )
        .bind(request_id.as_str())
        .bind(attempt_id.as_str())
        .bind(Json(error))
        .execute(&self.pool)
        .await
        .map_err(backend)?;
        Ok(result.rows_affected() == 1)
    }

    async fn mark_expired(
        &self,
        request_id: &ActivationRequestId,
    ) -> Result<bool, ActivationStoreError> {
        let result = sqlx::query(
            "UPDATE activation_requests SET state = 'expired' \
             WHERE id = $1 AND state IN ('pending','approved') AND expires_at <= NOW()",
        )
        .bind(request_id.as_str())
        .execute(&self.pool)
        .await
        .map_err(backend)?;
        Ok(result.rows_affected() == 1)
    }

    async fn list_applying(
        &self,
        guild_id: GuildId,
    ) -> Result<Vec<ActivationRequest>, ActivationStoreError> {
        let rows = sqlx::query_as::<_, ActivationRequestRow>(&format!(
            "SELECT {REQUEST_COLUMNS} FROM activation_requests \
             WHERE guild_id = $1 AND state = 'applying' ORDER BY id"
        ))
        .bind(guild_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;
        let mut connection = self.pool.acquire().await.map_err(backend)?;
        let mut requests = Vec::with_capacity(rows.len());
        for row in rows {
            let id = ActivationRequestId::parse(&row.id)
                .map_err(|error| backend(format!("invalid persisted id: {error}")))?;
            let approvals = sqlx::query_as::<_, ApprovalRow>(
                "SELECT approver_id, approved_at FROM activation_request_approvals \
                 WHERE request_id = $1 ORDER BY approver_id",
            )
            .bind(id.as_str())
            .fetch_all(&mut *connection)
            .await
            .map_err(backend)?;
            requests.push(decode_request(row, approvals)?);
        }
        Ok(requests)
    }

    async fn bookkeep_applied(
        &self,
        request_id: &ActivationRequestId,
        applied_by: UserId,
    ) -> Result<bool, ActivationStoreError> {
        let result = sqlx::query(
            "UPDATE activation_requests SET state = 'applied', apply_attempt_id = NULL, \
             apply_lease_until = NULL, last_apply_error = NULL, applied_at = NOW(), \
             applied_by = $2, completion_kind = 'crash_recovered', activation_notices = NULL \
             WHERE id = $1 AND state = 'applying'",
        )
        .bind(request_id.as_str())
        .bind(applied_by.to_string())
        .execute(&self.pool)
        .await
        .map_err(backend)?;
        Ok(result.rows_affected() == 1)
    }
}
