use std::future::Future;
use std::sync::Arc;

use automation_core::RunningRuleSetIdentity;
use automation_instance::{InstanceIdGenerator, InstanceRegistrarV1, InstanceRuleSetVersion};
use automation_instance_teardown::InstanceTeardownService;
use automation_ruleset::RuleSetVersion;
use automation_ruleset_dispatch::{GuildRoleSnapshotProvider, PinnedInstanceResolverV1};
use automation_runtime_registry::ExactServingRouteV1;
use resource_resolution::ResourceBindingMap;
use twilight_http::Client;
use twilight_model::application::interaction::Interaction;

use crate::mutation::TwilightMutationAdapter;
use crate::runner::{handle_interaction_with_resolver_v1, InteractionExecutionOutcomeV3};
use crate::shared_gateway_admission::SharedGatewayAdmittedInteractionV3;

#[allow(clippy::too_many_arguments)]
pub async fn execute_admitted_interaction_v3(
    mutation_http: &Client,
    interaction_http: &Client,
    admitted: SharedGatewayAdmittedInteractionV3,
    interaction: &Interaction,
    failure_message: &str,
    instances: &impl InstanceRegistrarV1,
    instance_ids: &impl InstanceIdGenerator,
    teardown: &impl InstanceTeardownService,
    pinned_resolver: &impl PinnedInstanceResolverV1,
    snapshot_provider: &impl GuildRoleSnapshotProvider,
) -> InteractionExecutionOutcomeV3 {
    let (identity, ruleset, bindings) = execution_inputs(admitted.route());
    let mutation = TwilightMutationAdapter::new(mutation_http, identity.key.clone());
    let handler = handle_interaction_with_resolver_v1(
        interaction_http,
        &identity,
        &mutation,
        &ruleset.definition,
        &bindings,
        interaction,
        failure_message,
        instances,
        instance_ids,
        teardown,
        pinned_resolver,
        snapshot_provider,
    );
    hold_admission_while(admitted, handler).await
}

fn execution_inputs(
    route: &ExactServingRouteV1,
) -> (
    RunningRuleSetIdentity,
    Arc<RuleSetVersion>,
    Arc<ResourceBindingMap>,
) {
    let artifact = Arc::clone(route.ruleset());
    let target = &route.identity().target;
    let identity = RunningRuleSetIdentity {
        key: target.ruleset_key.as_str().to_string(),
        version: InstanceRuleSetVersion::new(target.version.get())
            .expect("exact serving route version is non-zero"),
    };
    (identity, artifact, Arc::clone(route.bindings()))
}

async fn hold_admission_while<F>(
    admitted: SharedGatewayAdmittedInteractionV3,
    future: F,
) -> F::Output
where
    F: Future,
{
    let output = future.await;
    drop(admitted);
    output
}

#[cfg(test)]
mod tests {
    use std::num::{NonZeroU32, NonZeroUsize};

    use automation_instance::InMemoryInstanceStore;
    use automation_ruleset::{
        content_hash, RuleSetKey, RuleSetVersionId, CURRENT_RULESET_SCHEMA_VERSION,
    };
    use automation_runtime_convergence::{
        BindingRevision, FencingToken, ProcessInstanceId, RuntimeDeploymentTargetV1,
        RuntimeGeneration, RuntimeProcessIdentityV1,
    };
    use automation_runtime_registry::{
        ServingSlotKeyV1, ServingSlotRegistryConfigV1, ServingSlotRegistryV1,
    };
    use automation_state::InteractionRuleSet;
    use discord_model::{GuildId, UserId};
    use resource_resolution::{resource_binding_fingerprint_v2, ResourceBindingMap};

    use crate::custom_id::encode_button;
    use crate::shared_gateway_admission::{
        SharedGatewayAdmissionBudgetV3, SharedGatewayAdmissionConfigV3,
        SharedGatewayAdmissionErrorV3,
    };
    use crate::shared_gateway_control::{
        shared_gateway_control_channel_v3, GatewayControlConfigV3, GatewayReadyKindV3,
    };

    use super::*;

    fn exact_route(version: u32) -> ExactServingRouteV1 {
        let guild_id = GuildId(77);
        let ruleset_key = RuleSetKey::parse("studyroom").unwrap();
        let definition = InteractionRuleSet {
            version: 17,
            panels: Vec::new(),
            modals: Vec::new(),
            rules: Vec::new(),
        };
        let content_hash = content_hash(CURRENT_RULESET_SCHEMA_VERSION, &definition).unwrap();
        let bindings = ResourceBindingMap::default();
        let target = RuntimeDeploymentTargetV1 {
            guild_id,
            ruleset_key: ruleset_key.clone(),
            version: RuleSetVersionId::new(version).unwrap(),
            content_hash,
            binding_revision: BindingRevision::FIRST,
            binding_fingerprint: resource_binding_fingerprint_v2(&bindings),
        };
        let identity = RuntimeProcessIdentityV1 {
            target: target.clone(),
            runtime_generation: RuntimeGeneration::FIRST,
            process_instance_id: ProcessInstanceId::parse("shared-executor-test").unwrap(),
        };
        ExactServingRouteV1::new(
            identity,
            RuleSetVersion {
                guild_id,
                ruleset_key,
                version: target.version,
                schema_version: CURRENT_RULESET_SCHEMA_VERSION,
                definition,
                content_hash,
                created_by: UserId(9),
            },
            bindings,
        )
        .unwrap()
    }

    fn serving_registry(route: &ExactServingRouteV1) -> ServingSlotRegistryV1 {
        let registry = ServingSlotRegistryV1::new(ServingSlotRegistryConfigV1 {
            max_slots: NonZeroU32::new(2).unwrap(),
            max_active_interactions_per_slot: NonZeroU32::new(2).unwrap(),
            max_retired_routes_per_slot: NonZeroU32::new(2).unwrap(),
        });
        let token = registry
            .install(
                route.slot_key(),
                route.clone(),
                FencingToken::new(1).unwrap(),
            )
            .unwrap()
            .token;
        registry.activate(&token, route.identity()).unwrap();
        registry
    }

    #[test]
    fn execution_inputs_preserve_exact_route_identity_definition_and_bindings() {
        let route = exact_route(7);
        let (identity, artifact, bindings) = execution_inputs(&route);
        assert_eq!(identity.key, "studyroom");
        assert_eq!(identity.version.get(), 7);
        assert_eq!(artifact.definition.version, 17);
        assert!(Arc::ptr_eq(&artifact, route.ruleset()));
        assert!(Arc::ptr_eq(&bindings, route.bindings()));
    }

    #[tokio::test]
    async fn admission_is_held_until_controlled_handler_completes() {
        let route = exact_route(3);
        let key = ServingSlotKeyV1::from_target(&route.identity().target);
        let registry = serving_registry(&route);
        let alternate_registry = serving_registry(&route);
        let (control, mut runtime) =
            shared_gateway_control_channel_v3(GatewayControlConfigV3::default());
        let epoch = runtime.mark_connected(GatewayReadyKindV3::Ready).unwrap();
        let lease = control.issue_ready_lease(epoch).unwrap();
        let budget = SharedGatewayAdmissionBudgetV3::new(
            SharedGatewayAdmissionConfigV3::new(NonZeroUsize::new(1).unwrap()).unwrap(),
        );
        let admitted = budget
            .admit(
                &control,
                &lease,
                &registry,
                &InMemoryInstanceStore::new(),
                key.guild_id(),
                &encode_button(key.guild_id(), key.ruleset_key().as_str(), "join"),
            )
            .await
            .unwrap()
            .unwrap();
        let token = admitted.token().clone();
        let (started_sender, started_receiver) = tokio::sync::oneshot::channel();
        let (release_sender, release_receiver) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            hold_admission_while(admitted, async move {
                started_sender.send(()).unwrap();
                release_receiver.await.unwrap();
                41
            })
            .await
        });

        started_receiver.await.unwrap();
        let competing = budget
            .admit(
                &control,
                &lease,
                &alternate_registry,
                &InMemoryInstanceStore::new(),
                key.guild_id(),
                &encode_button(key.guild_id(), key.ruleset_key().as_str(), "join"),
            )
            .await;
        assert!(matches!(
            competing,
            Err(SharedGatewayAdmissionErrorV3::Overloaded)
        ));
        let drain = registry.begin_drain(&token).unwrap();
        assert_eq!(
            drain,
            automation_runtime_registry::SlotDrainOutcomeV1::DrainStarted {
                active_interactions: 1
            }
        );
        let pending = registry.observe_drain(&token).unwrap();
        assert_eq!(pending.active_interactions, 1);
        assert!(!pending.drained);

        release_sender.send(()).unwrap();
        assert_eq!(task.await.unwrap(), 41);
        let completed = registry.observe_drain(&token).unwrap();
        assert_eq!(completed.active_interactions, 0);
        assert!(completed.drained);
        let admitted_after_completion = budget
            .admit(
                &control,
                &lease,
                &alternate_registry,
                &InMemoryInstanceStore::new(),
                key.guild_id(),
                &encode_button(key.guild_id(), key.ruleset_key().as_str(), "join"),
            )
            .await
            .unwrap();
        assert!(admitted_after_completion.is_some());
    }
}
