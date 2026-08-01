use std::num::NonZeroU32;

use super::*;

#[derive(Default)]
struct RuntimeControllerFaultCohortCallsV2 {
    claim: usize,
    convergence_attempt: usize,
    hydration: usize,
    discord_preflight: usize,
    deployment_mutation: usize,
    discord_mutation: usize,
}

impl RuntimeControllerFaultCohortCallsV2 {
    fn enter_convergence_v2(&mut self) {
        self.convergence_attempt += 1;
        self.hydration += 1;
        self.discord_preflight += 1;
        self.deployment_mutation += 1;
        self.discord_mutation += 1;
    }

    fn enter_preflight_success_v2(&mut self) {
        self.deployment_mutation += 1;
        self.discord_mutation += 1;
    }

    fn enter_preflight_block_v2(&mut self) {
        self.deployment_mutation += 1;
    }
}

fn at(second: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(second, 0).unwrap()
}

fn claimed_receipt(installation_id: &str) -> RuntimeExecutionReceiptV1 {
    let identity: RuntimeDeploymentIdentityV1 = serde_json::from_value(serde_json::json!({
        "deployment_id": "deployment:d1-preflight",
        "tenant_id": "tenant:d1-preflight",
        "installation_id": installation_id,
        "promotion_id": "9".repeat(64),
        "activation_request_id": "activation:d1-preflight"
    }))
    .unwrap();
    let target: RuntimeDeploymentTargetV1 = serde_json::from_value(serde_json::json!({
        "guild_id": "4242",
        "ruleset_key": "studyroom",
        "version": 1,
        "content_hash": "8".repeat(64),
        "binding_revision": 1,
        "binding_fingerprint": "7".repeat(64)
    }))
    .unwrap();
    let mut deployment = RuntimeDeployment::request(
        identity,
        target,
        RuntimeGeneration::new(3).unwrap(),
        None,
        at(1),
    )
    .unwrap();
    let controller_id = ControllerId::parse("controller:d1-preflight").unwrap();
    let fencing_token = FencingToken::new(5).unwrap();
    deployment
        .acquire_lease(LeaseRequestV1 {
            expected_revision: deployment.revision(),
            controller_id: controller_id.clone(),
            fencing_token,
            now: at(10),
            expires_at: at(100),
        })
        .unwrap();
    RuntimeExecutionReceiptV1 {
        snapshot: deployment.snapshot(),
        controller_id,
        fencing_token,
        convergence_attempt: NonZeroU32::MIN,
        acquired_at: at(10),
        expires_at: at(100),
    }
}

#[test]
fn database_unavailable_before_claim_has_bounded_retry_and_zero_downstream_work() {
    let mut calls = RuntimeControllerFaultCohortCallsV2::default();
    calls.claim += 1;

    let gate =
        runtime_controller_claim_gate_v2(Err(RuntimeExecutionPersistenceErrorV1::Unavailable));
    let backoff = match gate {
        RuntimeControllerClaimGateV2::Wait(backoff) => backoff,
        RuntimeControllerClaimGateV2::Claimed(_) => {
            calls.enter_convergence_v2();
            panic!("unavailable database claim must not enter convergence")
        }
        RuntimeControllerClaimGateV2::Failed => {
            panic!("unavailable database claim must remain retryable")
        }
    };

    assert_eq!(backoff, Duration::from_secs(5));
    assert_eq!(calls.claim, 1);
    assert_eq!(calls.convergence_attempt, 0);
    assert_eq!(calls.hydration, 0);
    assert_eq!(calls.discord_preflight, 0);
    assert_eq!(calls.deployment_mutation, 0);
    assert_eq!(calls.discord_mutation, 0);
}

#[test]
fn discord_preflight_unavailable_after_claim_retains_scope_without_mutation_or_effects() {
    let receipt = claimed_receipt("installation:d1-preflight");
    let expected = receipt.clone();
    let expected_scope = RuntimeDeploymentScopeV1::from_identity(&receipt.snapshot.identity);
    let mut calls = RuntimeControllerFaultCohortCallsV2 {
        claim: 1,
        convergence_attempt: 1,
        hydration: 1,
        discord_preflight: 1,
        ..RuntimeControllerFaultCohortCallsV2::default()
    };

    let gate = runtime_controller_discord_preflight_gate_v2(
        Err(RuntimeDiscordPreflightErrorV2::Port(
            RuntimeControllerDiscordPreflightPortErrorV2::Discord(
                RuntimeDiscordPreflightErrorV1::Snapshot(
                    RuntimeReadinessSnapshotErrorV1::GuildRolesUnavailable,
                ),
            ),
        )),
        receipt,
    );
    let retained = match gate {
        RuntimeControllerDiscordPreflightGateV2::RetryRetained(receipt) => receipt,
        RuntimeControllerDiscordPreflightGateV2::Ready(_) => {
            calls.enter_preflight_success_v2();
            panic!("unavailable Discord preflight must not open the mutation path")
        }
        RuntimeControllerDiscordPreflightGateV2::DeploymentBlocked(_) => {
            calls.enter_preflight_block_v2();
            panic!("unavailable Discord preflight must not cancel the deployment")
        }
        RuntimeControllerDiscordPreflightGateV2::Failed => {
            panic!("unavailable Discord preflight must remain retryable")
        }
    };

    assert_eq!(*retained, expected);
    assert_eq!(
        RuntimeDeploymentScopeV1::from_identity(&retained.snapshot.identity),
        expected_scope
    );
    assert_eq!(calls.claim, 1);
    assert_eq!(calls.convergence_attempt, 1);
    assert_eq!(calls.hydration, 1);
    assert_eq!(calls.discord_preflight, 1);
    assert_eq!(calls.deployment_mutation, 0);
    assert_eq!(calls.discord_mutation, 0);
}
