use super::*;

fn promotion_id() -> PromotionId {
    PromotionId::parse(&"a".repeat(64)).unwrap()
}

fn approve_command() -> ApproveProductPromotionV1 {
    ApproveProductPromotionV1 {
        promotion: authoring_application::PromotionSelectorV1::new(promotion_id()),
        expected_payload_digest: ApprovalPayloadDigestV1::parse(&"c".repeat(64)).unwrap(),
        expected_revision: ProductRevisionV1::new(3).unwrap(),
        idempotency_key: ProductIdempotencyKeyV1::parse("approve-key").unwrap(),
    }
}

#[test]
fn product_request_id_is_bounded_validated_and_redacted() {
    assert_eq!(
        ProductRequestIdV1::parse("").unwrap_err(),
        ProductRequestIdError::Empty
    );
    assert_eq!(
        ProductRequestIdV1::parse(&"a".repeat(129)).unwrap_err(),
        ProductRequestIdError::TooLong
    );
    for invalid in ["request id", "요청", "request/id", "request\n"] {
        assert_eq!(
            ProductRequestIdV1::parse(invalid).unwrap_err(),
            ProductRequestIdError::InvalidCharacter
        );
    }
    let request_id = product_request_id();
    assert_eq!(request_id.as_str(), "req_01JZ7QW9YB2G8M4K6T3P5R1C0D");
    let debug = format!("{request_id:?}");
    assert_eq!(debug, "ProductRequestIdV1(<redacted>)");
    assert!(!debug.contains(request_id.as_str()));
}

fn exact_deployment() -> ExactDeploymentSelectorV1 {
    ExactDeploymentSelectorV1::from_server_projection(
        AutomationInstallationId::parse("installation-2").unwrap(),
        promotion_id(),
        "deployment-1",
        "b".repeat(64),
    )
    .unwrap()
}

fn decision_projection(phase: ProductDecisionPhaseV1) -> ProductDecisionProjectionV1 {
    ProductDecisionProjectionV1::from_server_projection(
        TenantId::parse("tenant-1").unwrap(),
        AutomationInstallationId::parse("installation-2").unwrap(),
        GuildId(900),
        promotion_id(),
        ProductRevisionV1::new(4).unwrap(),
        phase,
    )
}

struct Decisions {
    events: Arc<Mutex<Vec<&'static str>>>,
    phase: Mutex<ProductDecisionPhaseV1>,
    apply_phase: Mutex<ProductDecisionPhaseV1>,
    apply_exact_replay: Mutex<bool>,
}

impl ProductDecisionQueryPort<Evidence> for Decisions {
    async fn load_approval_preview(
        &self,
        _request: AuthorizedApprovalPreviewV1<'_, Evidence>,
    ) -> Result<ProductApprovalPreviewV1, ProductControlPortError> {
        panic!("preview is not used by this fixture")
    }

    async fn load_product_status(
        &self,
        request: AuthorizedProductStatusV1<'_, Evidence>,
    ) -> Result<ProductDecisionProjectionV1, ProductControlPortError> {
        self.events.lock().unwrap().push("decision_status");
        assert_eq!(request.actor().principal_id().as_str(), "principal-1");
        assert_eq!(request.scope().acting_user_id(), UserId(200));
        assert_eq!(request.evidence(), &Evidence("fresh-authority-evidence"));
        assert_eq!(request.promotion().promotion_id(), &promotion_id());
        Ok(decision_projection(self.phase.lock().unwrap().clone()))
    }
}

impl ProductApprovalPort<Evidence> for Decisions {
    async fn approve_payload_bound(
        &self,
        request: AuthorizedApproveProductV1<'_, Evidence>,
    ) -> Result<ProductMutationReceiptV1, ProductControlPortError> {
        self.events.lock().unwrap().push("approve_payload_bound");
        assert_eq!(request.request_id(), &product_request_id());
        assert_eq!(request.actor().principal_id().as_str(), "principal-1");
        assert_eq!(request.session_fingerprint().as_bytes(), &[7_u8; 32]);
        assert_eq!(request.scope().acting_user_id(), UserId(200));
        assert_eq!(request.evidence(), &Evidence("fresh-authority-evidence"));
        assert_eq!(
            format!("{:?}", request.context()),
            "ProductMutationContextV1(<redacted>)"
        );
        assert_eq!(
            request.command().expected_payload_digest.as_str(),
            "c".repeat(64)
        );
        assert_eq!(request.command().expected_revision.get(), 3);
        assert_eq!(request.command().idempotency_key.as_str(), "approve-key");
        Ok(ProductMutationReceiptV1::from_server_projection(
            decision_projection(self.phase.lock().unwrap().clone()),
            false,
        ))
    }
}

impl ProductRejectionPort<Evidence> for Decisions {
    async fn reject_payload_bound(
        &self,
        request: AuthorizedRejectProductV1<'_, Evidence>,
    ) -> Result<ProductMutationReceiptV1, ProductControlPortError> {
        self.events.lock().unwrap().push("reject_payload_bound");
        assert_eq!(request.request_id(), &product_request_id());
        assert_eq!(request.session_fingerprint().as_bytes(), &[7_u8; 32]);
        assert_eq!(request.scope().acting_user_id(), UserId(200));
        assert_eq!(request.evidence(), &Evidence("fresh-authority-evidence"));
        assert_eq!(request.command().reason.as_str(), "unsafe requested scope");
        assert_eq!(request.command().idempotency_key.as_str(), "reject-key");
        Ok(ProductMutationReceiptV1::from_server_projection(
            decision_projection(ProductDecisionPhaseV1::Rejected),
            false,
        ))
    }
}

impl ProductApplyPort<Evidence> for Decisions {
    async fn apply_idempotent(
        &self,
        request: AuthorizedApplyProductV1<'_, Evidence>,
    ) -> Result<ProductMutationReceiptV1, ProductControlPortError> {
        self.events.lock().unwrap().push("apply_idempotent");
        assert_eq!(request.request_id(), &product_request_id());
        assert_eq!(request.actor().principal_id().as_str(), "principal-1");
        assert_eq!(request.session_fingerprint().as_bytes(), &[7_u8; 32]);
        assert_eq!(request.scope().acting_user_id(), UserId(200));
        assert_eq!(request.evidence(), &Evidence("fresh-authority-evidence"));
        assert_eq!(
            request.command().expected_payload_digest.as_str(),
            "c".repeat(64)
        );
        assert_eq!(request.command().expected_revision.get(), 3);
        assert_eq!(request.command().idempotency_key.as_str(), "apply-key");
        Ok(ProductMutationReceiptV1::from_server_projection(
            decision_projection(self.apply_phase.lock().unwrap().clone()),
            *self.apply_exact_replay.lock().unwrap(),
        ))
    }
}

fn assert_compatibility_decision_port<T: ProductDecisionPort<Evidence>>() {}

struct Deployments {
    events: Arc<Mutex<Vec<&'static str>>>,
    status: Mutex<DeploymentStatusProjectionV1>,
}

impl DeploymentStatusPort<Evidence> for Deployments {
    async fn load_exact_deployment_status(
        &self,
        request: AuthorizedDeploymentStatusV1<'_, Evidence>,
    ) -> Result<DeploymentStatusProjectionV1, DeploymentStatusPortError> {
        self.events.lock().unwrap().push("deployment_status");
        assert_eq!(request.actor().principal_id().as_str(), "principal-1");
        assert_eq!(request.scope().installation_id().as_str(), "installation-2");
        assert_eq!(request.evidence(), &Evidence("fresh-authority-evidence"));
        assert_eq!(request.exact_deployment(), &exact_deployment());
        Ok(self.status.lock().unwrap().clone())
    }
}

fn product_fixture(
    phase: ProductDecisionPhaseV1,
    deployment: DeploymentStatusProjectionV1,
) -> (
    Arc<Mutex<Vec<&'static str>>>,
    Authentication,
    GuildAuthority,
    Decisions,
    Deployments,
) {
    let events = Arc::new(Mutex::new(Vec::new()));
    (
        events.clone(),
        Authentication {
            events: events.clone(),
            failure: None,
        },
        GuildAuthority {
            events: events.clone(),
            failure: None,
        },
        Decisions {
            events: events.clone(),
            phase: Mutex::new(phase),
            apply_phase: Mutex::new(ProductDecisionPhaseV1::Applied {
                exact_deployment: exact_deployment(),
            }),
            apply_exact_replay: Mutex::new(false),
        },
        Deployments {
            events: events.clone(),
            status: Mutex::new(deployment),
        },
    )
}

struct ObservationDecisions {
    preview: ProductApprovalPreviewObservationV1,
    status: ProductDecisionObservationV1,
}

impl ProductDecisionObservationPort<Evidence> for ObservationDecisions {
    async fn load_approval_preview_observation(
        &self,
        _request: AuthorizedApprovalPreviewV1<'_, Evidence>,
    ) -> Result<ProductApprovalPreviewObservationV1, ProductControlPortError> {
        Ok(self.preview.clone())
    }

    async fn load_product_status_observation(
        &self,
        _request: AuthorizedProductStatusV1<'_, Evidence>,
    ) -> Result<ProductDecisionObservationV1, ProductControlPortError> {
        Ok(self.status.clone())
    }
}

struct ObservationDeployments {
    status: DeploymentStatusObservationV1,
}

struct OperationalDecisions {
    status: ProductDecisionObservationV1,
}

impl ProductDecisionObservationPort<Evidence> for OperationalDecisions {
    async fn load_approval_preview_observation(
        &self,
        _request: AuthorizedApprovalPreviewV1<'_, Evidence>,
    ) -> Result<ProductApprovalPreviewObservationV1, ProductControlPortError> {
        panic!("preview is not used by the operational fixture")
    }

    async fn load_product_status_observation(
        &self,
        _request: AuthorizedProductStatusV1<'_, Evidence>,
    ) -> Result<ProductDecisionObservationV1, ProductControlPortError> {
        Ok(self.status.clone())
    }
}

struct OperationalDeployments {
    events: Arc<Mutex<Vec<&'static str>>>,
    status: DeploymentOperationalObservationV2,
}

impl DeploymentOperationalStatusPortV2<Evidence> for OperationalDeployments {
    async fn load_exact_deployment_operational_status_v2(
        &self,
        request: AuthorizedDeploymentStatusV1<'_, Evidence>,
    ) -> Result<DeploymentOperationalObservationV2, DeploymentStatusPortError> {
        self.events
            .lock()
            .unwrap()
            .push("deployment_operational_v2");
        assert_eq!(request.actor().principal_id().as_str(), "principal-1");
        assert_eq!(request.scope().installation_id().as_str(), "installation-2");
        assert_eq!(request.evidence(), &Evidence("fresh-authority-evidence"));
        assert_eq!(request.exact_deployment(), &exact_deployment());
        Ok(self.status.clone())
    }
}

impl DeploymentStatusObservationPort<Evidence> for ObservationDeployments {
    async fn load_exact_deployment_observation(
        &self,
        _request: AuthorizedDeploymentStatusV1<'_, Evidence>,
    ) -> Result<DeploymentStatusObservationV1, DeploymentStatusPortError> {
        Ok(self.status.clone())
    }
}

#[test]
fn operational_status_v2_uses_only_the_server_derived_exact_deployment() {
    block_on(async {
        let decision_observed_at = UNIX_EPOCH + Duration::from_secs(4_000_000_000);
        let runtime_observed_at = decision_observed_at + Duration::from_secs(1);
        let decisions = OperationalDecisions {
            status: ProductDecisionObservationV1::from_server_projection(
                decision_projection(ProductDecisionPhaseV1::Applied {
                    exact_deployment: exact_deployment(),
                }),
                decision_observed_at,
            ),
        };
        let runtime = DeploymentOperationalObservationV2::from_server_projection(
            DeploymentStatusObservationV1::from_server_projection(
                DeploymentStatusProjectionV1::Pending,
                runtime_observed_at,
                None,
                None,
            )
            .unwrap(),
            DeploymentOperationalProjectionV2 {
                phase: DeploymentConvergencePhaseV2::Requested,
                current_attempt: 0,
                last_failure_attempt: None,
                retry: None,
                operator_action: None,
                attestation: None,
                serving: DeploymentServingFreshnessV2::NotExpected,
            },
        )
        .unwrap();
        let events = Arc::new(Mutex::new(Vec::new()));
        let deployments = OperationalDeployments {
            events: events.clone(),
            status: runtime,
        };
        let authentication = Authentication {
            events: events.clone(),
            failure: None,
        };
        let authority = GuildAuthority {
            events: events.clone(),
            failure: None,
        };
        let application =
            ProductControlApplication::new(&authentication, &authority, &decisions, &deployments);
        let result = application
            .get_deployment_operational_status_v2(
                "opaque-session-token",
                &installation(),
                RuntimeDeploymentQueryV1 {
                    promotion: authoring_application::PromotionSelectorV1::new(promotion_id()),
                },
            )
            .await
            .unwrap();
        assert_eq!(
            result.status(),
            &authoring_application::DeploymentStatusV1::Pending
        );
        assert_eq!(result.decision_observed_at(), decision_observed_at);
        let deployment = result.deployment().unwrap();
        assert_eq!(deployment.observed_at(), runtime_observed_at);
        assert_eq!(deployment.current_attempt(), 0);
        assert_eq!(
            events.lock().unwrap().as_slice(),
            ["authenticate", "authorize", "deployment_operational_v2"]
        );
    });
}

async fn exact_preview_observation(
    observed_at: SystemTime,
) -> (
    authoring_application::PromotionSelectorV1,
    ProductApprovalPreviewObservationV1,
) {
    let input = StartPromotionV1 {
        idempotency_key: IdempotencyKey::parse("observation-promotion-key").unwrap(),
        context: AuthenticatedPromotionContext {
            tenant_id: TenantId::parse("tenant-1").unwrap(),
            principal_id: PrincipalId::parse("principal-1").unwrap(),
            session_owner_id: PrincipalId::parse("principal-1").unwrap(),
            session_id: AuthoringSessionId::parse("session-1").unwrap(),
            session_generation: SessionGeneration::new(7).unwrap(),
            guild_id: GuildId(900),
            installation_id: AutomationInstallationId::parse("installation-2").unwrap(),
            ruleset_key: "studyrooms".parse().unwrap(),
            requester: UserId(200),
            binding_revision: BindingRevision::new(3).unwrap(),
            policy: ApprovalPolicyV1 {
                revision: PolicyRevision::new(5).unwrap(),
                required_approvals: NonZeroU32::new(2).unwrap(),
                ttl_seconds: NonZeroU64::new(3600).unwrap(),
            },
        },
        artifact: artifact().await,
    };
    let submission = run_test_promotion(input).await.unwrap();
    let record = match &submission.advancement {
        ResumePromotionOutcomeV1::Advanced(record)
        | ResumePromotionOutcomeV1::AlreadyActivationPending(record)
        | ResumePromotionOutcomeV1::TerminalExpired(record) => record,
    };
    let payload = record.product_approval_payload().unwrap();
    let digest = approval_payload_digest_v1(&payload).unwrap();
    let activation_expires_at = match &record.stage {
        authoring_promotion::PromotionStageV1::ActivationPending { activation, .. }
        | authoring_promotion::PromotionStageV1::Expired { activation, .. } => {
            activation.expires_at.into()
        }
        _ => panic!("unexpected promotion stage"),
    };
    let selector = authoring_application::PromotionSelectorV1::new(record.id.clone());
    let preview = ProductApprovalPreviewV1::from_server_projection(
        AutomationInstallationId::parse("installation-2").unwrap(),
        GuildId(900),
        payload,
        ApprovalPayloadDigestV1::parse(digest.as_str()).unwrap(),
        ProductRevisionV1::new(1).unwrap(),
        ProductDecisionPhaseV1::PendingApproval,
    );
    (
        selector,
        ProductApprovalPreviewObservationV1::from_server_projection(
            preview,
            activation_expires_at,
            observed_at,
        ),
    )
}

#[test]
fn exact_preview_and_status_observations_preserve_database_time_and_revision() {
    block_on(async {
        let observed_at = UNIX_EPOCH + Duration::from_secs(4_000_000_000);
        let (selector, preview) = exact_preview_observation(observed_at).await;
        let decision = ProductDecisionProjectionV1::from_server_projection(
            TenantId::parse("tenant-1").unwrap(),
            AutomationInstallationId::parse("installation-2").unwrap(),
            GuildId(900),
            selector.promotion_id().clone(),
            ProductRevisionV1::new(7).unwrap(),
            ProductDecisionPhaseV1::PendingApproval,
        );
        let decisions = ObservationDecisions {
            preview: preview.clone(),
            status: ProductDecisionObservationV1::from_server_projection(decision, observed_at),
        };
        let deployments = ObservationDeployments {
            status: DeploymentStatusObservationV1::from_server_projection(
                DeploymentStatusProjectionV1::Pending,
                observed_at,
                None,
                None,
            )
            .unwrap(),
        };
        let events = Arc::new(Mutex::new(Vec::new()));
        let authentication = Authentication {
            events: events.clone(),
            failure: None,
        };
        let authority = GuildAuthority {
            events,
            failure: None,
        };
        let application =
            ProductControlApplication::new(&authentication, &authority, &decisions, &deployments);
        let preview_result = application
            .get_approval_preview_observation(
                "opaque-session-token",
                &installation(),
                ProductStatusQueryV1 {
                    promotion: selector.clone(),
                },
            )
            .await
            .unwrap();
        assert_eq!(preview_result.observed_at(), observed_at);
        assert_eq!(
            preview_result.activation_expires_at(),
            preview.activation_expires_at()
        );
        let status = application
            .get_product_status_observation(
                "opaque-session-token",
                &installation(),
                ProductStatusQueryV1 {
                    promotion: selector,
                },
            )
            .await
            .unwrap();
        assert_eq!(status.status(), ProductStatusV1::PendingApproval);
        assert_eq!(status.decision().revision().get(), 7);
        assert_eq!(status.decision_observed_at(), observed_at);
        assert!(status.deployment().is_none());
    });
}

#[test]
fn live_observation_preserves_attestation_heartbeat_and_lease_ordering() {
    block_on(async {
        let decision_observed_at = UNIX_EPOCH + Duration::from_secs(100);
        let runtime_observed_at = UNIX_EPOCH + Duration::from_secs(120);
        let last_heartbeat_at = UNIX_EPOCH + Duration::from_secs(110);
        let lease_expires_at = UNIX_EPOCH + Duration::from_secs(130);
        let exact = exact_deployment();
        let decision = decision_projection(ProductDecisionPhaseV1::Applied {
            exact_deployment: exact.clone(),
        });
        let (_, preview) = exact_preview_observation(UNIX_EPOCH + Duration::from_secs(1)).await;
        let decisions = ObservationDecisions {
            preview,
            status: ProductDecisionObservationV1::from_server_projection(
                decision,
                decision_observed_at,
            ),
        };
        let deployments = ObservationDeployments {
            status: DeploymentStatusObservationV1::from_server_projection(
                DeploymentStatusProjectionV1::ExactLive(
                    ExactLiveProjectionV1::from_exact_attestation(
                        exact,
                        NonZeroU64::new(9).unwrap(),
                    ),
                ),
                runtime_observed_at,
                Some(last_heartbeat_at),
                Some(lease_expires_at),
            )
            .unwrap(),
        };
        let events = Arc::new(Mutex::new(Vec::new()));
        let authentication = Authentication {
            events: events.clone(),
            failure: None,
        };
        let authority = GuildAuthority {
            events,
            failure: None,
        };
        let observation =
            ProductControlApplication::new(&authentication, &authority, &decisions, &deployments)
                .get_product_status_observation(
                    "opaque-session-token",
                    &installation(),
                    ProductStatusQueryV1 {
                        promotion: authoring_application::PromotionSelectorV1::new(promotion_id()),
                    },
                )
                .await
                .unwrap();
        assert_eq!(observation.status(), ProductStatusV1::Live);
        assert_eq!(observation.decision().revision().get(), 4);
        let runtime = observation.deployment().unwrap();
        assert_eq!(runtime.observed_at(), runtime_observed_at);
        assert_eq!(runtime.last_heartbeat_at(), Some(last_heartbeat_at));
        assert_eq!(runtime.lease_expires_at(), Some(lease_expires_at));
        assert!(last_heartbeat_at <= runtime.observed_at());
        assert!(runtime.observed_at() < lease_expires_at);
    });
}

#[test]
fn runtime_observation_before_decision_observation_fails_closed() {
    block_on(async {
        let exact = exact_deployment();
        let decision_observed_at = UNIX_EPOCH + Duration::from_secs(120);
        let runtime_observed_at = UNIX_EPOCH + Duration::from_secs(110);
        let (_, preview) = exact_preview_observation(UNIX_EPOCH + Duration::from_secs(1)).await;
        let decisions = ObservationDecisions {
            preview,
            status: ProductDecisionObservationV1::from_server_projection(
                decision_projection(ProductDecisionPhaseV1::Applied {
                    exact_deployment: exact,
                }),
                decision_observed_at,
            ),
        };
        let deployments = ObservationDeployments {
            status: DeploymentStatusObservationV1::from_server_projection(
                DeploymentStatusProjectionV1::Pending,
                runtime_observed_at,
                None,
                None,
            )
            .unwrap(),
        };
        let events = Arc::new(Mutex::new(Vec::new()));
        let authentication = Authentication {
            events: events.clone(),
            failure: None,
        };
        let authority = GuildAuthority {
            events,
            failure: None,
        };
        assert_eq!(
            ProductControlApplication::new(&authentication, &authority, &decisions, &deployments)
                .get_product_status_observation(
                    "opaque-session-token",
                    &installation(),
                    ProductStatusQueryV1 {
                        promotion: authoring_application::PromotionSelectorV1::new(promotion_id()),
                    },
                )
                .await
                .unwrap_err(),
            ProductApplicationError::InvalidProjection
        );
    });
}

#[test]
fn deployment_failure_observation_exposes_only_stable_bounded_metadata() {
    let observed_at = UNIX_EPOCH + Duration::from_secs(120);
    let observation = DeploymentStatusObservationV1::from_server_projection(
        DeploymentStatusProjectionV1::Failed {
            retryable: true,
            failure_code: "gateway_start_failed".to_string(),
        },
        observed_at,
        None,
        None,
    )
    .unwrap();
    let failure = observation.failure().unwrap();
    assert!(failure.retryable());
    assert_eq!(failure.failure_code().as_str(), "gateway_start_failed");
    assert!(DeploymentFailureCodeV1::parse("private_internal_identifier").is_err());
    assert!(DeploymentStatusObservationV1::from_server_projection(
        DeploymentStatusProjectionV1::Failed {
            retryable: false,
            failure_code: "private_internal_identifier".to_string(),
        },
        observed_at,
        None,
        None,
    )
    .is_err());
}

#[test]
fn approval_supports_pending_quorum_and_approved_outcomes() {
    block_on(async {
        assert_compatibility_decision_port::<Decisions>();
        for phase in [
            ProductDecisionPhaseV1::PendingApproval,
            ProductDecisionPhaseV1::Approved,
        ] {
            let (events, authentication, authority, decisions, deployments) =
                product_fixture(phase.clone(), DeploymentStatusProjectionV1::NotRequested);
            let application = ProductControlApplication::new(
                &authentication,
                &authority,
                &decisions,
                &deployments,
            );
            let receipt = application
                .approve(
                    "opaque-session-token",
                    "csrf-proof",
                    &product_request_id(),
                    &installation(),
                    approve_command(),
                )
                .await
                .unwrap();
            assert!(!receipt.exact_replay());
            assert_eq!(receipt.projection().phase(), &phase);
            assert_eq!(
                *events.lock().unwrap(),
                vec![
                    "authenticate_mutation",
                    "authorize",
                    "approve_payload_bound"
                ]
            );
        }
    });
}

#[test]
fn approval_rejects_non_approval_success_projection() {
    block_on(async {
        let (_, authentication, authority, decisions, deployments) = product_fixture(
            ProductDecisionPhaseV1::Rejected,
            DeploymentStatusProjectionV1::NotRequested,
        );
        let error =
            ProductControlApplication::new(&authentication, &authority, &decisions, &deployments)
                .approve(
                    "opaque-session-token",
                    "csrf-proof",
                    &product_request_id(),
                    &installation(),
                    approve_command(),
                )
                .await
                .unwrap_err();
        assert_eq!(error, ProductApplicationError::InvalidProjection);
    });
}

#[test]
fn reject_reason_is_bounded_and_digest_bound_port_is_used() {
    assert_eq!(
        RejectionReasonV1::parse("  ").unwrap_err(),
        RejectionReasonError::Empty
    );
    assert_eq!(
        RejectionReasonV1::parse(&"x".repeat(1_001)).unwrap_err(),
        RejectionReasonError::TooLong
    );
    assert!(RejectionReasonV1::parse(&"😀".repeat(1_000)).is_ok());
    assert_eq!(
        RejectionReasonV1::parse("line\nbreak").unwrap_err(),
        RejectionReasonError::ControlCharacter
    );
    block_on(async {
        let (events, authentication, authority, decisions, deployments) = product_fixture(
            ProductDecisionPhaseV1::PendingApproval,
            DeploymentStatusProjectionV1::NotRequested,
        );
        ProductControlApplication::new(&authentication, &authority, &decisions, &deployments)
            .reject(
                "opaque-session-token",
                "csrf-proof",
                &product_request_id(),
                &installation(),
                RejectProductPromotionV1 {
                    promotion: authoring_application::PromotionSelectorV1::new(promotion_id()),
                    expected_payload_digest: ApprovalPayloadDigestV1::parse(&"c".repeat(64))
                        .unwrap(),
                    expected_revision: ProductRevisionV1::new(3).unwrap(),
                    idempotency_key: ProductIdempotencyKeyV1::parse("reject-key").unwrap(),
                    reason: RejectionReasonV1::parse("unsafe requested scope").unwrap(),
                },
            )
            .await
            .unwrap();
        assert_eq!(
            *events.lock().unwrap(),
            vec!["authenticate_mutation", "authorize", "reject_payload_bound"]
        );
    });
}

#[test]
fn applied_pointer_is_runtime_pending_until_exact_live_attestation() {
    block_on(async {
        let (events, authentication, authority, decisions, deployments) = product_fixture(
            ProductDecisionPhaseV1::Applied {
                exact_deployment: exact_deployment(),
            },
            DeploymentStatusProjectionV1::Pending,
        );
        let application =
            ProductControlApplication::new(&authentication, &authority, &decisions, &deployments);
        let query = ProductStatusQueryV1 {
            promotion: authoring_application::PromotionSelectorV1::new(promotion_id()),
        };
        assert_eq!(
            application
                .get_product_status("opaque-session-token", &installation(), query.clone())
                .await
                .unwrap(),
            ProductStatusV1::RuntimePending
        );
        *deployments.status.lock().unwrap() =
            DeploymentStatusProjectionV1::ExactLive(ExactLiveProjectionV1::from_exact_attestation(
                exact_deployment(),
                NonZeroU64::new(9).unwrap(),
            ));
        assert_eq!(
            application
                .get_product_status("opaque-session-token", &installation(), query)
                .await
                .unwrap(),
            ProductStatusV1::Live
        );
        assert_eq!(
            events
                .lock()
                .unwrap()
                .iter()
                .filter(|event| **event == "deployment_status")
                .count(),
            2
        );
    });
}

#[test]
fn apply_passes_no_attempt_identifier_and_reports_runtime_pending() {
    block_on(async {
        let (events, authentication, authority, decisions, deployments) = product_fixture(
            ProductDecisionPhaseV1::Approved,
            DeploymentStatusProjectionV1::NotRequested,
        );
        let result =
            ProductControlApplication::new(&authentication, &authority, &decisions, &deployments)
                .apply(
                    "opaque-session-token",
                    "csrf-proof",
                    &product_request_id(),
                    &installation(),
                    ApplyProductPromotionV1 {
                        promotion: authoring_application::PromotionSelectorV1::new(promotion_id()),
                        expected_payload_digest: ApprovalPayloadDigestV1::parse(&"c".repeat(64))
                            .unwrap(),
                        expected_revision: ProductRevisionV1::new(3).unwrap(),
                        idempotency_key: ProductIdempotencyKeyV1::parse("apply-key").unwrap(),
                    },
                )
                .await
                .unwrap();
        assert_eq!(result.status(), ProductStatusV1::RuntimePending);
        assert!(!result.exact_replay());
        assert_eq!(result.exact_deployment(), &exact_deployment());
        assert_eq!(
            *events.lock().unwrap(),
            vec!["authenticate_mutation", "authorize", "apply_idempotent"]
        );
    });
}

#[test]
fn apply_surfaces_terminal_supersession_without_querying_runtime() {
    block_on(async {
        let (events, authentication, authority, decisions, deployments) = product_fixture(
            ProductDecisionPhaseV1::Approved,
            DeploymentStatusProjectionV1::NotRequested,
        );
        *decisions.apply_phase.lock().unwrap() = ProductDecisionPhaseV1::Superseded;
        let error =
            ProductControlApplication::new(&authentication, &authority, &decisions, &deployments)
                .apply(
                    "opaque-session-token",
                    "csrf-proof",
                    &product_request_id(),
                    &installation(),
                    ApplyProductPromotionV1 {
                        promotion: authoring_application::PromotionSelectorV1::new(promotion_id()),
                        expected_payload_digest: ApprovalPayloadDigestV1::parse(&"c".repeat(64))
                            .unwrap(),
                        expected_revision: ProductRevisionV1::new(3).unwrap(),
                        idempotency_key: ProductIdempotencyKeyV1::parse("apply-key").unwrap(),
                    },
                )
                .await
                .unwrap_err();
        assert_eq!(
            error,
            ProductApplicationError::Control(ProductControlPortError::Superseded)
        );
        assert_eq!(
            *events.lock().unwrap(),
            vec!["authenticate_mutation", "authorize", "apply_idempotent"]
        );
    });
}

#[test]
fn exact_apply_replay_may_project_live_after_runtime_verification() {
    block_on(async {
        let (events, authentication, authority, decisions, deployments) = product_fixture(
            ProductDecisionPhaseV1::Approved,
            DeploymentStatusProjectionV1::ExactLive(ExactLiveProjectionV1::from_exact_attestation(
                exact_deployment(),
                NonZeroU64::new(7).unwrap(),
            )),
        );
        *decisions.apply_exact_replay.lock().unwrap() = true;
        let result =
            ProductControlApplication::new(&authentication, &authority, &decisions, &deployments)
                .apply(
                    "opaque-session-token",
                    "csrf-proof",
                    &product_request_id(),
                    &installation(),
                    ApplyProductPromotionV1 {
                        promotion: authoring_application::PromotionSelectorV1::new(promotion_id()),
                        expected_payload_digest: ApprovalPayloadDigestV1::parse(&"c".repeat(64))
                            .unwrap(),
                        expected_revision: ProductRevisionV1::new(3).unwrap(),
                        idempotency_key: ProductIdempotencyKeyV1::parse("apply-key").unwrap(),
                    },
                )
                .await
                .unwrap();
        assert_eq!(result.status(), ProductStatusV1::Live);
        assert!(result.exact_replay());
        assert_eq!(result.exact_deployment(), &exact_deployment());
        assert_eq!(
            *events.lock().unwrap(),
            vec![
                "authenticate_mutation",
                "authorize",
                "apply_idempotent",
                "deployment_status"
            ]
        );
    });
}

#[test]
fn mismatched_exact_live_projection_is_rejected() {
    block_on(async {
        let wrong = ExactDeploymentSelectorV1::from_server_projection(
            AutomationInstallationId::parse("installation-2").unwrap(),
            promotion_id(),
            "deployment-wrong",
            "b".repeat(64),
        )
        .unwrap();
        let (_, authentication, authority, decisions, deployments) = product_fixture(
            ProductDecisionPhaseV1::Applied {
                exact_deployment: exact_deployment(),
            },
            DeploymentStatusProjectionV1::ExactLive(ExactLiveProjectionV1::from_exact_attestation(
                wrong,
                NonZeroU64::new(2).unwrap(),
            )),
        );
        let result =
            ProductControlApplication::new(&authentication, &authority, &decisions, &deployments)
                .get_product_status(
                    "opaque-session-token",
                    &installation(),
                    ProductStatusQueryV1 {
                        promotion: authoring_application::PromotionSelectorV1::new(promotion_id()),
                    },
                )
                .await;
        assert_eq!(
            result.unwrap_err(),
            ProductApplicationError::InvalidProjection
        );
    });
}

#[test]
fn deployment_status_requires_the_server_derived_exact_target() {
    block_on(async {
        let (_, authentication, authority, decisions, deployments) = product_fixture(
            ProductDecisionPhaseV1::Applied {
                exact_deployment: exact_deployment(),
            },
            DeploymentStatusProjectionV1::Pending,
        );
        let result =
            ProductControlApplication::new(&authentication, &authority, &decisions, &deployments)
                .get_deployment_status(
                    "opaque-session-token",
                    &installation(),
                    RuntimeDeploymentQueryV1 {
                        promotion: authoring_application::PromotionSelectorV1::new(promotion_id()),
                    },
                )
                .await
                .unwrap();
        assert_eq!(result, authoring_application::DeploymentStatusV1::Pending);
    });
}

#[test]
fn deployment_failure_code_is_closed_and_cannot_leak_backend_text() {
    for failure_code in [
        "Discord said no: secret detail",
        "",
        &"a".repeat(65),
        "private_internal_identifier",
    ] {
        assert!(DeploymentFailureCodeV1::parse(failure_code).is_err());
    }
    for failure_code in [
        DeploymentFailureCodeV1::RuntimeEnvironmentUnavailable,
        DeploymentFailureCodeV1::ActivationNotObservable,
        DeploymentFailureCodeV1::PanelReconciliationFailed,
        DeploymentFailureCodeV1::GatewayStartFailed,
        DeploymentFailureCodeV1::GatewayReadyTimeout,
        DeploymentFailureCodeV1::RuntimeInvariantViolation,
        DeploymentFailureCodeV1::DeploymentBlocked,
        DeploymentFailureCodeV1::ActiveTargetChanged,
        DeploymentFailureCodeV1::BindingAuthorityChanged,
        DeploymentFailureCodeV1::ProductAuthorityInactive,
        DeploymentFailureCodeV1::ProductAuthorityNotCurrent,
        DeploymentFailureCodeV1::DeploymentSuperseded,
        DeploymentFailureCodeV1::DeploymentCancelled,
    ] {
        assert_eq!(
            DeploymentFailureCodeV1::parse(failure_code.as_str()).unwrap(),
            failure_code
        );
    }
    block_on(async {
        let (_, authentication, authority, decisions, deployments) = product_fixture(
            ProductDecisionPhaseV1::Applied {
                exact_deployment: exact_deployment(),
            },
            DeploymentStatusProjectionV1::Failed {
                retryable: true,
                failure_code: "private_internal_identifier".to_string(),
            },
        );
        let error =
            ProductControlApplication::new(&authentication, &authority, &decisions, &deployments)
                .get_deployment_status(
                    "opaque-session-token",
                    &installation(),
                    RuntimeDeploymentQueryV1 {
                        promotion: authoring_application::PromotionSelectorV1::new(promotion_id()),
                    },
                )
                .await
                .unwrap_err();
        assert_eq!(error, ProductApplicationError::InvalidProjection);
    });
}

#[test]
fn product_candidate_failures_have_bounded_stable_codes() {
    let cases = [
        (
            ProductCandidateErrorCodeV1::TargetCorrupt,
            "product target artifact is corrupt",
        ),
        (
            ProductCandidateErrorCodeV1::BindingRevisionUnavailable,
            "authoritative product binding revision is unavailable",
        ),
        (
            ProductCandidateErrorCodeV1::UnsupportedSchema,
            "product target schema is unsupported",
        ),
        (
            ProductCandidateErrorCodeV1::StructurallyInvalid,
            "product target structure is invalid",
        ),
        (
            ProductCandidateErrorCodeV1::HashComputationFailed,
            "product target hash could not be verified",
        ),
        (
            ProductCandidateErrorCodeV1::HashMismatch,
            "product target hash does not match its content",
        ),
        (
            ProductCandidateErrorCodeV1::BindingInvalid,
            "product target bindings are invalid",
        ),
        (
            ProductCandidateErrorCodeV1::BlockingPolicy,
            "product target violates a blocking policy",
        ),
        (
            ProductCandidateErrorCodeV1::MissingCapabilities,
            "product target requires unavailable capabilities",
        ),
        (
            ProductCandidateErrorCodeV1::RoleHierarchyUnavailable,
            "product target role hierarchy evidence is unavailable",
        ),
        (
            ProductCandidateErrorCodeV1::RoleHierarchyIncomplete,
            "product target role hierarchy evidence is incomplete",
        ),
        (
            ProductCandidateErrorCodeV1::RoleUnmanageable,
            "product target requires a role the bot cannot manage",
        ),
    ];
    for (code, message) in cases {
        assert_eq!(code.to_string(), message);
        assert_eq!(
            ProductControlPortError::InvalidServerCandidate(code).to_string(),
            format!("server-owned product candidate is invalid: {message}")
        );
    }
    assert_eq!(
        ProductControlPortError::Superseded.to_string(),
        "promotion was superseded by newer server state"
    );
}
