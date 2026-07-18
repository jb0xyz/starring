use automation_ruleset_activation::{
    ActivationApprovalContextV1, ActivationDigest, ActivationLinkStateV1, ActivationRequest,
    ActivationRequestId, ActivationRequestStore, ActivationStoreError, ApplyAttemptId,
    ApplyErrorRecord, ApprovalDecisionError, ApproveError, ClaimDecision, ClaimOutcome,
    CompletionKind, CreateActivationRequest, CreateProductActivationRequest, LinkDecision,
    LinkDecisionError, LinkProductActivation, LinkProductError, RejectError,
    RejectionDecisionError, SupersessionReasonV1, WithdrawDecisionError, WithdrawError,
};
use chrono::{DateTime, Utc};
use discord_model::{GuildId, UserId};
use sqlx::postgres::PgPool;
use sqlx::types::Json;
use sqlx::{PgConnection, Row};

use crate::row::{
    authority_kind, backend, completion_kind_str, decode_request, link_state_name, state_str,
    ActivationRequestRow, ApprovalRow, REQUEST_COLUMNS,
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
        "SELECT approver_id, approved_at, approval_payload_digest FROM activation_request_approvals \
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
    sqlx::query_scalar("SELECT clock_timestamp()")
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
    sqlx::query_as(
        "SELECT captured_at, captured_at + ($1 * INTERVAL '1 second') \
         FROM (SELECT clock_timestamp() AS captured_at) AS clock",
    )
    .bind(lease_seconds)
    .fetch_one(connection)
    .await
    .map_err(backend)
}

async fn bind_product_executor(
    connection: &mut PgConnection,
    request: &ActivationRequest,
) -> Result<(), ActivationStoreError> {
    let ActivationApprovalContextV1::ProductAuthoring { context } = &request.approval_context
    else {
        return Ok(());
    };
    let bound = sqlx::query_scalar::<_, String>(
        "SELECT set_config('starring.product_approval_context_digest', $1, TRUE)",
    )
    .bind(context.approval_context_digest.as_str())
    .fetch_one(connection)
    .await
    .map_err(backend)?;
    if bound == context.approval_context_digest.as_str() {
        Ok(())
    } else {
        Err(backend("product activation executor binding mismatch"))
    }
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
        ClaimDecision::Unlinked => ClaimOutcome::Unlinked,
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
             SELECT $1, $2, $3, $4, $5, $6, $7, 'pending', captured_at, \
             captured_at + ($8 * INTERVAL '1 millisecond'), $9, $10 \
             FROM (SELECT clock_timestamp() AS captured_at) AS clock",
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

    async fn create_product(
        &self,
        input: CreateProductActivationRequest,
    ) -> Result<ActivationRequest, ActivationStoreError> {
        let validated =
            ActivationRequest::create_product(input.clone(), DateTime::<Utc>::UNIX_EPOCH)
                .map_err(ActivationStoreError::InvalidRequest)?;
        let ttl_millis = i64::try_from(input.context.policy.ttl_seconds.get())
            .ok()
            .and_then(|seconds| seconds.checked_mul(1000))
            .ok_or_else(|| {
                ActivationStoreError::InvalidRequest("request ttl overflow".to_string())
            })?;
        let observed_version = validated
            .observed_active
            .as_ref()
            .map(|observed| i64::from(observed.version.get()));
        let observed_hash = validated
            .observed_active
            .as_ref()
            .map(|observed| observed.content_hash.to_hex());
        let approval_context = ActivationApprovalContextV1::ProductAuthoring {
            context: Box::new(input.context.clone()),
        };
        let link_state = ActivationLinkStateV1::Unlinked;
        let mut tx = self.pool.begin().await.map_err(backend)?;
        let result = sqlx::query(
            "INSERT INTO activation_requests (id, guild_id, ruleset_key, target_version, \
             target_content_hash, requester_id, required_approvals, state, created_at, expires_at, \
             observed_active_version, observed_active_hash, authority_kind, link_state_name, \
             approval_context, link_state, promotion_id, promotion_request_digest, \
             approval_payload_digest, approval_context_digest) \
             SELECT $1, $2, $3, $4, $5, $6, $7, 'pending', captured_at, \
             captured_at + ($8 * INTERVAL '1 millisecond'), $9, $10, $11, $12, $13, $14, \
             $15, $16, $17, $18 \
             FROM (SELECT clock_timestamp() AS captured_at) AS clock",
        )
        .bind(input.id.as_str())
        .bind(input.target.guild_id.to_string())
        .bind(input.target.ruleset_key.as_str())
        .bind(i64::from(input.target.version.get()))
        .bind(input.target.content_hash.to_hex())
        .bind(input.requester.to_string())
        .bind(
            i32::try_from(input.context.policy.required_approvals.get()).map_err(|_| {
                ActivationStoreError::InvalidRequest("required approvals overflow".to_string())
            })?,
        )
        .bind(ttl_millis)
        .bind(observed_version)
        .bind(observed_hash)
        .bind(authority_kind(&approval_context))
        .bind(link_state_name(&link_state))
        .bind(Json(&approval_context))
        .bind(Json(&link_state))
        .bind(input.context.promotion_id.as_str())
        .bind(input.context.promotion_request_digest.as_str())
        .bind(input.context.approval_payload_digest.as_str())
        .bind(input.context.approval_context_digest.as_str())
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

    async fn link_product(
        &self,
        request_id: &ActivationRequestId,
        link: LinkProductActivation,
    ) -> Result<ActivationRequest, LinkProductError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        let mut request = fetch_request(&mut tx, request_id, true)
            .await?
            .ok_or(ActivationStoreError::NotFound)?;
        let now = database_now(&mut tx).await?;
        let decision = match request.link_product_at(
            &link.promotion_id,
            &link.promotion_request_digest,
            &link.approval_context_digest,
            now,
        ) {
            Ok(decision) => decision,
            Err(LinkDecisionError::Expired) => {
                sqlx::query("UPDATE activation_requests SET state = 'expired' WHERE id = $1")
                    .bind(request_id.as_str())
                    .execute(&mut *tx)
                    .await
                    .map_err(backend)?;
                tx.commit().await.map_err(backend)?;
                return Err(LinkProductError::Expired);
            }
            Err(error) => {
                tx.rollback().await.map_err(backend)?;
                return Err(error.into());
            }
        };
        if decision == LinkDecision::ExactReplay {
            if request.state == automation_ruleset_activation::ActivationRequestState::Expired {
                sqlx::query(
                    "UPDATE activation_requests SET state = 'expired' \
                     WHERE id = $1 AND state IN ('pending','approved')",
                )
                .bind(request_id.as_str())
                .execute(&mut *tx)
                .await
                .map_err(backend)?;
            }
            tx.commit().await.map_err(backend)?;
            return Ok(request);
        }
        let ActivationLinkStateV1::Linked { linked_at } = &request.link_state else {
            tx.rollback().await.map_err(backend)?;
            return Err(LinkProductError::Store(ActivationStoreError::Backend(
                "linked request omitted link timestamp".to_string(),
            )));
        };
        let result = sqlx::query(
            "UPDATE activation_requests SET link_state_name = 'linked', link_state = $2, \
             linked_at = $3 WHERE id = $1 AND state = 'pending' \
             AND link_state_name = 'unlinked' AND expires_at > $3",
        )
        .bind(request_id.as_str())
        .bind(Json(&request.link_state))
        .bind(*linked_at)
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        if result.rows_affected() != 1 {
            tx.rollback().await.map_err(backend)?;
            return Err(LinkProductError::Store(ActivationStoreError::Backend(
                "product activation link CAS failed".to_string(),
            )));
        }
        let linked = fetch_request(&mut tx, request_id, false)
            .await?
            .ok_or(ActivationStoreError::NotFound)?;
        tx.commit().await.map_err(backend)?;
        Ok(linked)
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
        let request = fetch_request(&mut tx, request_id, true).await?;
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
            "INSERT INTO activation_request_approvals \
             (request_id, approver_id, approved_at, approval_payload_digest) \
             VALUES ($1, $2, $3, NULL)",
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

    async fn approve_bound(
        &self,
        request_id: &ActivationRequestId,
        approver: UserId,
        approval_payload_digest: &ActivationDigest,
    ) -> Result<ActivationRequest, ApproveError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        let mut request = fetch_request(&mut tx, request_id, true)
            .await?
            .ok_or(ActivationStoreError::NotFound)?;
        let now = database_now(&mut tx).await?;
        if let Err(error) = request.approve_bound_at(approver, approval_payload_digest, now) {
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
            "INSERT INTO activation_request_approvals \
             (request_id, approver_id, approved_at, approval_payload_digest) \
             VALUES ($1, $2, $3, $4)",
        )
        .bind(request_id.as_str())
        .bind(approver.to_string())
        .bind(approval.approved_at)
        .bind(approval_payload_digest.as_str())
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

    async fn withdraw(
        &self,
        request_id: &ActivationRequestId,
        withdrawn_by: UserId,
        reason: String,
    ) -> Result<ActivationRequest, WithdrawError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        let mut request = fetch_request(&mut tx, request_id, true)
            .await?
            .ok_or(ActivationStoreError::NotFound)?;
        let now = database_now(&mut tx).await?;
        if let Err(error) = request.withdraw_at(withdrawn_by, reason, now) {
            if error == WithdrawDecisionError::Expired {
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
        let termination = request
            .termination
            .as_ref()
            .ok_or_else(|| backend("withdrawal omitted terminal evidence"))?;
        let result = sqlx::query(
            "UPDATE activation_requests SET state = 'withdrawn', termination = $2, \
             last_apply_error = NULL \
             WHERE id = $1 AND state IN ('pending','approved')",
        )
        .bind(request_id.as_str())
        .bind(Json(termination))
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        if result.rows_affected() != 1 {
            tx.rollback().await.map_err(backend)?;
            return Err(WithdrawError::Store(backend("withdrawal CAS failed")));
        }
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
        bind_product_executor(&mut tx, &request).await?;
        let update = sqlx::query(
            "UPDATE activation_requests SET state = 'applying', apply_attempt_id = $2, \
             apply_attempt_no = apply_attempt_no + 1, \
             apply_lease_until = $3 \
             WHERE id = $1 AND state = 'approved' AND expires_at > $4",
        )
        .bind(request_id.as_str())
        .bind(attempt_id.as_str())
        .bind(lease_until)
        .bind(now)
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
        bind_product_executor(&mut tx, &request).await?;
        let result = sqlx::query(
            "UPDATE activation_requests SET apply_attempt_id = $2, \
             apply_attempt_no = apply_attempt_no + 1, \
             apply_lease_until = $3 \
             WHERE id = $1 AND state = 'applying' AND apply_lease_until <= $4",
        )
        .bind(request_id.as_str())
        .bind(attempt_id.as_str())
        .bind(lease_until)
        .bind(now)
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

    async fn supersede_applying(
        &self,
        request_id: &ActivationRequestId,
        attempt_id: &ApplyAttemptId,
        reason: SupersessionReasonV1,
    ) -> Result<bool, ActivationStoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        let mut request = fetch_request(&mut tx, request_id, true)
            .await?
            .ok_or(ActivationStoreError::NotFound)?;
        let now = database_now(&mut tx).await?;
        if !request.supersede_at(attempt_id, reason, now) {
            tx.rollback().await.map_err(backend)?;
            return Ok(false);
        }
        let termination = request
            .termination
            .as_ref()
            .ok_or_else(|| backend("supersession omitted terminal evidence"))?;
        let result = sqlx::query(
            "UPDATE activation_requests SET state = 'superseded', apply_attempt_id = NULL, \
             apply_lease_until = NULL, last_apply_error = NULL, termination = $3 \
             WHERE id = $1 AND state = 'applying' AND apply_attempt_id = $2",
        )
        .bind(request_id.as_str())
        .bind(attempt_id.as_str())
        .bind(Json(termination))
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        if result.rows_affected() != 1 {
            tx.rollback().await.map_err(backend)?;
            return Ok(false);
        }
        let stored = fetch_request(&mut tx, request_id, false)
            .await?
            .ok_or(ActivationStoreError::NotFound)?;
        if stored != request {
            tx.rollback().await.map_err(backend)?;
            return Err(backend("supersession persistence mismatch"));
        }
        tx.commit().await.map_err(backend)?;
        Ok(true)
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
                "SELECT approver_id, approved_at, approval_payload_digest \
                 FROM activation_request_approvals \
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
