use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use automation_core::{
    AdapterError, AdapterErrorKind, CreateChannelSpec, CreateRoleSpec, DiscordMutationAdapter,
    InteractionResponder, ModalPresentation, PostPanelSpec,
};
use automation_state::{ModalFieldSpec, ModalFieldStyle, ModalInputPolicy};
use discord_model::{ChannelId, GuildId, MessageId, OverwriteTarget, Permissions, RoleId, UserId};

use super::*;

#[derive(Debug)]
struct SecretPermitError(&'static str);

struct FakePermitV1 {
    trace: Arc<Mutex<Vec<&'static str>>>,
    fail_intent: bool,
    fail_result: bool,
    fail_execution: bool,
    intent_dispositions: Mutex<VecDeque<InteractionInitialResponseIntentDispositionV1>>,
    intents: Mutex<Vec<InteractionInitialResponseIntentV1>>,
    results: Mutex<Vec<InteractionInitialResponseResultV1>>,
}

impl FakePermitV1 {
    fn new(trace: Arc<Mutex<Vec<&'static str>>>) -> Self {
        Self {
            trace,
            fail_intent: false,
            fail_result: false,
            fail_execution: false,
            intent_dispositions: Mutex::new(VecDeque::new()),
            intents: Mutex::new(Vec::new()),
            results: Mutex::new(Vec::new()),
        }
    }

    fn with_failures(
        trace: Arc<Mutex<Vec<&'static str>>>,
        fail_intent: bool,
        fail_result: bool,
        fail_execution: bool,
    ) -> Self {
        Self {
            trace,
            fail_intent,
            fail_result,
            fail_execution,
            intent_dispositions: Mutex::new(VecDeque::new()),
            intents: Mutex::new(Vec::new()),
            results: Mutex::new(Vec::new()),
        }
    }

    fn with_intent_dispositions(
        trace: Arc<Mutex<Vec<&'static str>>>,
        dispositions: impl IntoIterator<Item = InteractionInitialResponseIntentDispositionV1>,
    ) -> Self {
        Self {
            trace,
            fail_intent: false,
            fail_result: false,
            fail_execution: false,
            intent_dispositions: Mutex::new(dispositions.into_iter().collect()),
            intents: Mutex::new(Vec::new()),
            results: Mutex::new(Vec::new()),
        }
    }
}

impl InteractionEffectPermitV1 for FakePermitV1 {
    type Error = SecretPermitError;

    async fn commit_initial_response_intent_v1(
        &self,
        intent: &InteractionInitialResponseIntentV1,
    ) -> Result<InteractionInitialResponseIntentDispositionV1, Self::Error> {
        self.trace.lock().unwrap().push("permit.initial_intent");
        if self.fail_intent {
            return Err(SecretPermitError("permit-intent-secret"));
        }
        self.intents.lock().unwrap().push(intent.clone());
        Ok(self
            .intent_dispositions
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(InteractionInitialResponseIntentDispositionV1::ExternalCallAuthorized))
    }

    async fn commit_initial_response_result_v1(
        &self,
        result: &InteractionInitialResponseResultV1,
    ) -> Result<(), Self::Error> {
        self.trace.lock().unwrap().push("permit.initial_result");
        if self.fail_result {
            return Err(SecretPermitError("permit-result-secret"));
        }
        self.results.lock().unwrap().push(result.clone());
        Ok(())
    }

    async fn commit_idempotent_execution_intent_v1(&self) -> Result<(), Self::Error> {
        self.trace.lock().unwrap().push("permit.execution_intent");
        if self.fail_execution {
            return Err(SecretPermitError("permit-execution-secret"));
        }
        Ok(())
    }
}

struct FakeResponderV1 {
    trace: Arc<Mutex<Vec<&'static str>>>,
    initial_error: Option<AdapterError>,
    edit_error: Option<AdapterError>,
}

impl FakeResponderV1 {
    fn successful(trace: Arc<Mutex<Vec<&'static str>>>) -> Self {
        Self {
            trace,
            initial_error: None,
            edit_error: None,
        }
    }

    fn initial_failure(trace: Arc<Mutex<Vec<&'static str>>>, error: AdapterError) -> Self {
        Self {
            trace,
            initial_error: Some(error),
            edit_error: None,
        }
    }

    fn initial_result(&self) -> Result<(), AdapterError> {
        self.initial_error.clone().map_or(Ok(()), Err)
    }
}

impl InteractionResponder for FakeResponderV1 {
    async fn respond_ephemeral(&self, _: String) -> Result<(), AdapterError> {
        self.trace.lock().unwrap().push("external.respond");
        self.initial_result()
    }

    async fn open_modal(&self, _: &ModalPresentation) -> Result<(), AdapterError> {
        self.trace.lock().unwrap().push("external.modal");
        self.initial_result()
    }

    async fn defer_ephemeral(&self) -> Result<(), AdapterError> {
        self.trace.lock().unwrap().push("external.defer");
        self.initial_result()
    }

    async fn edit_response(&self, _: String) -> Result<(), AdapterError> {
        self.trace.lock().unwrap().push("external.edit");
        self.edit_error.clone().map_or(Ok(()), Err)
    }
}

struct FakeMutationV1 {
    trace: Arc<Mutex<Vec<&'static str>>>,
}

impl DiscordMutationAdapter for FakeMutationV1 {
    async fn grant_role(&self, _: GuildId, _: UserId, _: RoleId) -> Result<(), AdapterError> {
        self.trace.lock().unwrap().push("external.grant_role");
        Ok(())
    }

    async fn create_channel(
        &self,
        _: GuildId,
        _: CreateChannelSpec,
    ) -> Result<ChannelId, AdapterError> {
        self.trace.lock().unwrap().push("external.create_channel");
        Ok(ChannelId(301))
    }

    async fn create_role(&self, _: GuildId, _: CreateRoleSpec) -> Result<RoleId, AdapterError> {
        self.trace.lock().unwrap().push("external.create_role");
        Ok(RoleId(302))
    }

    async fn upsert_overwrite(
        &self,
        _: GuildId,
        _: ChannelId,
        _: OverwriteTarget,
        _: Permissions,
        _: Permissions,
    ) -> Result<(), AdapterError> {
        self.trace.lock().unwrap().push("external.upsert_overwrite");
        Ok(())
    }

    async fn post_panel(
        &self,
        _: GuildId,
        _: ChannelId,
        _: PostPanelSpec,
    ) -> Result<MessageId, AdapterError> {
        self.trace.lock().unwrap().push("external.post_panel");
        Ok(MessageId(303))
    }
}

fn modal(title: &str, label: &str) -> ModalPresentation {
    ModalPresentation {
        key: "room_modal".to_string(),
        title: title.to_string(),
        fields: vec![ModalFieldSpec {
            key: "room_name".to_string(),
            label: label.to_string(),
            style: ModalFieldStyle::Short,
            required: true,
            min_length: Some(2),
            max_length: Some(40),
            input_policy: ModalInputPolicy::TrimUnicodeWhitespace,
        }],
    }
}

#[tokio::test]
async fn every_initial_response_commits_intent_then_external_then_result() {
    let trace = Arc::new(Mutex::new(Vec::new()));
    let permit = FakePermitV1::new(Arc::clone(&trace));
    let responder = FakeResponderV1::successful(Arc::clone(&trace));
    let fenced = ReceiptFencedInteractionResponderV1::new(&responder, &permit);

    fenced.respond_ephemeral("ready".to_string()).await.unwrap();
    fenced
        .open_modal(&modal("Create room", "Room name"))
        .await
        .unwrap();
    fenced.defer_ephemeral().await.unwrap();

    assert_eq!(
        *trace.lock().unwrap(),
        [
            "permit.initial_intent",
            "external.respond",
            "permit.initial_result",
            "permit.initial_intent",
            "external.modal",
            "permit.initial_result",
            "permit.initial_intent",
            "external.defer",
            "permit.initial_result",
        ]
    );
    assert_eq!(
        permit
            .intents
            .lock()
            .unwrap()
            .iter()
            .map(InteractionInitialResponseIntentV1::kind)
            .collect::<Vec<_>>(),
        [
            InteractionInitialResponseKindV1::RespondEphemeral,
            InteractionInitialResponseKindV1::OpenModal,
            InteractionInitialResponseKindV1::DeferEphemeral,
        ]
    );
    assert!(permit
        .results
        .lock()
        .unwrap()
        .iter()
        .all(|result| result.result() == InteractionInitialResponseResultKindV1::Succeeded));
}

#[tokio::test]
async fn initial_intent_outage_produces_zero_external_calls() {
    let trace = Arc::new(Mutex::new(Vec::new()));
    let permit = FakePermitV1::with_failures(Arc::clone(&trace), true, false, false);
    let responder = FakeResponderV1::successful(Arc::clone(&trace));
    let fenced = ReceiptFencedInteractionResponderV1::new(&responder, &permit);

    let error = fenced
        .respond_ephemeral("never-send".to_string())
        .await
        .unwrap_err();

    assert_eq!(*trace.lock().unwrap(), ["permit.initial_intent"]);
    assert_eq!(error.kind, AdapterErrorKind::Unknown);
    assert_eq!(error.message, RECEIPT_PERSISTENCE_FAILURE_MESSAGE_V1);
    assert!(!format!("{error:?}").contains("permit-intent-secret"));
}

#[tokio::test]
async fn identical_response_action_executes_one_discord_call_and_one_result_commit() {
    let trace = Arc::new(Mutex::new(Vec::new()));
    let permit = FakePermitV1::with_intent_dispositions(
        Arc::clone(&trace),
        [
            InteractionInitialResponseIntentDispositionV1::ExternalCallAuthorized,
            InteractionInitialResponseIntentDispositionV1::ExactReplaySuppressed,
        ],
    );
    let responder = FakeResponderV1::successful(Arc::clone(&trace));
    let fenced = ReceiptFencedInteractionResponderV1::new(&responder, &permit);

    fenced.respond_ephemeral("same".to_string()).await.unwrap();
    fenced.respond_ephemeral("same".to_string()).await.unwrap();

    assert_eq!(
        *trace.lock().unwrap(),
        [
            "permit.initial_intent",
            "external.respond",
            "permit.initial_result",
            "permit.initial_intent",
        ]
    );
    assert_eq!(permit.intents.lock().unwrap().len(), 2);
    assert_eq!(permit.results.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn succeeded_response_exact_replay_suppresses_discord_and_result_recommit() {
    let trace = Arc::new(Mutex::new(Vec::new()));
    let permit = FakePermitV1::with_intent_dispositions(
        Arc::clone(&trace),
        [InteractionInitialResponseIntentDispositionV1::ExactReplaySuppressed],
    );
    let operation = encode_initial_response_operation_v1(InitialResponsePayloadV1::Respond(
        "already-succeeded",
    ));
    let intent = build_initial_response_intent_v1(
        InteractionInitialResponseKindV1::RespondEphemeral,
        &operation,
    );
    permit
        .results
        .lock()
        .unwrap()
        .push(build_initial_response_result_v1(
            intent.digest.clone(),
            InteractionInitialResponseResultKindV1::Succeeded,
            &operation,
        ));
    let responder = FakeResponderV1::successful(Arc::clone(&trace));
    let fenced = ReceiptFencedInteractionResponderV1::new(&responder, &permit);

    fenced
        .respond_ephemeral("already-succeeded".to_string())
        .await
        .unwrap();

    assert_eq!(*trace.lock().unwrap(), ["permit.initial_intent"]);
    assert_eq!(permit.results.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn canonical_response_digests_are_deterministic_and_exact_payload_sensitive() {
    let trace = Arc::new(Mutex::new(Vec::new()));
    let permit = FakePermitV1::new(Arc::clone(&trace));
    let responder = FakeResponderV1::successful(trace);
    let fenced = ReceiptFencedInteractionResponderV1::new(&responder, &permit);

    fenced.respond_ephemeral("alpha".to_string()).await.unwrap();
    fenced.respond_ephemeral("alpha".to_string()).await.unwrap();
    fenced.respond_ephemeral("beta".to_string()).await.unwrap();
    fenced
        .open_modal(&modal("Create room", "Room name"))
        .await
        .unwrap();
    fenced
        .open_modal(&modal("Create room", "Topic name"))
        .await
        .unwrap();
    fenced.defer_ephemeral().await.unwrap();

    let intents = permit.intents.lock().unwrap();
    assert_eq!(intents[0].digest(), intents[1].digest());
    assert_ne!(intents[0].digest(), intents[2].digest());
    assert_ne!(intents[3].digest(), intents[4].digest());
    assert_ne!(intents[0].digest(), intents[5].digest());
    let results = permit.results.lock().unwrap();
    assert_eq!(results[0].digest(), results[1].digest());
    assert_ne!(results[0].digest(), results[2].digest());
    assert_ne!(results[3].digest(), results[4].digest());
    assert_ne!(results[0].digest(), results[5].digest());
}

#[tokio::test]
async fn network_and_unknown_results_are_indeterminate_without_backend_text() {
    let first_trace = Arc::new(Mutex::new(Vec::new()));
    let first_permit = FakePermitV1::new(Arc::clone(&first_trace));
    let first_responder = FakeResponderV1::initial_failure(
        Arc::clone(&first_trace),
        AdapterError::new(AdapterErrorKind::Network, "backend-network-secret-a"),
    );
    let first = ReceiptFencedInteractionResponderV1::new(&first_responder, &first_permit);
    let first_error = first
        .respond_ephemeral("same-payload".to_string())
        .await
        .unwrap_err();

    let second_trace = Arc::new(Mutex::new(Vec::new()));
    let second_permit = FakePermitV1::new(Arc::clone(&second_trace));
    let second_responder = FakeResponderV1::initial_failure(
        second_trace,
        AdapterError::new(AdapterErrorKind::Network, "backend-network-secret-b"),
    );
    let second = ReceiptFencedInteractionResponderV1::new(&second_responder, &second_permit);
    second
        .respond_ephemeral("same-payload".to_string())
        .await
        .unwrap_err();

    let first_results = first_permit.results.lock().unwrap();
    let second_results = second_permit.results.lock().unwrap();
    assert_eq!(
        first_results[0].result(),
        InteractionInitialResponseResultKindV1::Indeterminate
    );
    assert_eq!(first_results[0].digest(), second_results[0].digest());
    assert_eq!(first_error.kind, AdapterErrorKind::Network);
    assert_eq!(first_error.message, INITIAL_RESPONSE_FAILURE_MESSAGE_V1);
    let rendered = format!("{first_error:?}{:?}", first_results[0]);
    assert!(!rendered.contains("backend-network-secret-a"));
    assert!(!rendered.contains("same-payload"));

    let unknown = Err(AdapterError::new(
        AdapterErrorKind::Unknown,
        "backend-unknown-secret",
    ));
    let explicit = Err(AdapterError::new(
        AdapterErrorKind::BadRequest,
        "backend-http-secret",
    ));
    assert_eq!(
        classify_initial_response_result_v1(&unknown),
        InteractionInitialResponseResultKindV1::Indeterminate
    );
    assert_eq!(
        classify_initial_response_result_v1(&explicit),
        InteractionInitialResponseResultKindV1::DefinitiveFailure
    );
}

#[tokio::test]
async fn result_persistence_failure_after_external_attempt_stops_with_redacted_error() {
    let trace = Arc::new(Mutex::new(Vec::new()));
    let permit = FakePermitV1::with_failures(Arc::clone(&trace), false, true, false);
    let responder = FakeResponderV1::successful(Arc::clone(&trace));
    let fenced = ReceiptFencedInteractionResponderV1::new(&responder, &permit);

    let error = fenced.defer_ephemeral().await.unwrap_err();

    assert_eq!(
        *trace.lock().unwrap(),
        [
            "permit.initial_intent",
            "external.defer",
            "permit.initial_result"
        ]
    );
    assert_eq!(error.kind, AdapterErrorKind::Unknown);
    assert_eq!(error.message, RECEIPT_PERSISTENCE_FAILURE_MESSAGE_V1);
    assert!(!format!("{error:?}").contains("permit-result-secret"));
}

#[tokio::test]
async fn edit_and_every_mutation_are_execution_intent_fenced() {
    let trace = Arc::new(Mutex::new(Vec::new()));
    let permit = FakePermitV1::new(Arc::clone(&trace));
    let responder = FakeResponderV1::successful(Arc::clone(&trace));
    let mutation = FakeMutationV1 {
        trace: Arc::clone(&trace),
    };
    let fenced_responder = ReceiptFencedInteractionResponderV1::new(&responder, &permit);
    let fenced_mutation = ReceiptFencedDiscordMutationAdapterV1::new(&mutation, &permit);

    fenced_responder
        .edit_response("complete".to_string())
        .await
        .unwrap();
    fenced_mutation
        .grant_role(GuildId(1), UserId(2), RoleId(3))
        .await
        .unwrap();
    fenced_mutation
        .create_channel(
            GuildId(1),
            CreateChannelSpec {
                name: "study".to_string(),
            },
        )
        .await
        .unwrap();
    fenced_mutation
        .create_role(
            GuildId(1),
            CreateRoleSpec {
                name: "student".to_string(),
            },
        )
        .await
        .unwrap();
    fenced_mutation
        .upsert_overwrite(
            GuildId(1),
            ChannelId(4),
            OverwriteTarget::Role(RoleId(3)),
            Permissions::VIEW_CHANNEL,
            Permissions::SEND_MESSAGES,
        )
        .await
        .unwrap();
    fenced_mutation
        .post_panel(
            GuildId(1),
            ChannelId(4),
            PostPanelSpec {
                content: "Join".to_string(),
                buttons: Vec::new(),
            },
        )
        .await
        .unwrap();

    assert_eq!(
        *trace.lock().unwrap(),
        [
            "permit.execution_intent",
            "external.edit",
            "permit.execution_intent",
            "external.grant_role",
            "permit.execution_intent",
            "external.create_channel",
            "permit.execution_intent",
            "external.create_role",
            "permit.execution_intent",
            "external.upsert_overwrite",
            "permit.execution_intent",
            "external.post_panel",
        ]
    );
}

#[tokio::test]
async fn execution_intent_outage_prevents_edit_and_mutation_calls() {
    let edit_trace = Arc::new(Mutex::new(Vec::new()));
    let edit_permit = FakePermitV1::with_failures(Arc::clone(&edit_trace), false, false, true);
    let responder = FakeResponderV1::successful(Arc::clone(&edit_trace));
    let fenced_responder = ReceiptFencedInteractionResponderV1::new(&responder, &edit_permit);
    let edit_error = fenced_responder
        .edit_response("never-edit".to_string())
        .await
        .unwrap_err();
    assert_eq!(*edit_trace.lock().unwrap(), ["permit.execution_intent"]);
    assert!(!format!("{edit_error:?}").contains("permit-execution-secret"));

    let mutation_trace = Arc::new(Mutex::new(Vec::new()));
    let mutation_permit =
        FakePermitV1::with_failures(Arc::clone(&mutation_trace), false, false, true);
    let mutation = FakeMutationV1 {
        trace: Arc::clone(&mutation_trace),
    };
    let fenced_mutation = ReceiptFencedDiscordMutationAdapterV1::new(&mutation, &mutation_permit);
    fenced_mutation
        .grant_role(GuildId(1), UserId(2), RoleId(3))
        .await
        .unwrap_err();
    assert_eq!(*mutation_trace.lock().unwrap(), ["permit.execution_intent"]);
}

#[tokio::test]
async fn execution_errors_preserve_kind_without_backend_text() {
    let trace = Arc::new(Mutex::new(Vec::new()));
    let permit = FakePermitV1::new(Arc::clone(&trace));
    let responder = FakeResponderV1 {
        trace: Arc::clone(&trace),
        initial_error: None,
        edit_error: Some(AdapterError::new(
            AdapterErrorKind::Forbidden,
            "backend-execution-secret",
        )),
    };
    let fenced = ReceiptFencedInteractionResponderV1::new(&responder, &permit);

    let error = fenced
        .edit_response("never-visible".to_string())
        .await
        .unwrap_err();

    assert_eq!(
        *trace.lock().unwrap(),
        ["permit.execution_intent", "external.edit"]
    );
    assert_eq!(error.kind, AdapterErrorKind::Forbidden);
    assert_eq!(error.message, EXECUTION_FAILURE_MESSAGE_V1);
    assert!(!format!("{error:?}").contains("backend-execution-secret"));
}

#[test]
fn public_debug_surfaces_are_redacted() {
    let trace = Arc::new(Mutex::new(Vec::new()));
    let permit = FakePermitV1::new(Arc::clone(&trace));
    let responder = FakeResponderV1::successful(Arc::clone(&trace));
    let mutation = FakeMutationV1 { trace };
    let fenced_responder = ReceiptFencedInteractionResponderV1::new(&responder, &permit);
    let fenced_mutation = ReceiptFencedDiscordMutationAdapterV1::new(&mutation, &permit);
    let operation = encode_initial_response_operation_v1(InitialResponsePayloadV1::Respond(
        "private-response-payload",
    ));
    let intent = build_initial_response_intent_v1(
        InteractionInitialResponseKindV1::RespondEphemeral,
        &operation,
    );
    let result = build_initial_response_result_v1(
        intent.digest.clone(),
        InteractionInitialResponseResultKindV1::Succeeded,
        &operation,
    );

    for rendered in [
        format!("{fenced_responder:?}"),
        format!("{fenced_mutation:?}"),
        format!("{intent:?}"),
        format!("{:?}", intent.digest()),
        format!("{result:?}"),
        format!("{:?}", result.digest()),
    ] {
        assert!(rendered.contains("<redacted>"));
        assert!(!rendered.contains("private-response-payload"));
    }

    let secret_error = SecretPermitError("permit-debug-secret");
    assert_eq!(secret_error.0, "permit-debug-secret");
}
