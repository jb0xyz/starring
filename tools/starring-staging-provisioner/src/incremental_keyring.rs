use crate::crypto::{
    generate_incremental_interaction_token_envelope_keyring, KeyringSecretV1, RandomSourceV1,
    SecretItemRefV1, SystemRandomSourceV1,
};
use crate::identity::{
    INTERACTION_TOKEN_ENVELOPE_KEYRING_IDENTITY, PRODUCT_ACTION_KEYRING_IDENTITY,
    SNAPSHOT_ENVELOPE_KEYRING_IDENTITY,
};
use crate::keychain::KeychainClientV1;
use crate::keyring::{validate_api_keyring_pair, validate_keyring_set};
use crate::{postgres_environment_is_present, ProvisionerErrorV1, StagingAcknowledgementV1};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IncrementalInteractionTokenKeyringOutcomeV1 {
    Created,
    ExactReplay,
}

impl IncrementalInteractionTokenKeyringOutcomeV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::ExactReplay => "exact_replay",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IncrementalInteractionTokenKeyringReportV1 {
    outcome: IncrementalInteractionTokenKeyringOutcomeV1,
    active_key_id: String,
}

impl IncrementalInteractionTokenKeyringReportV1 {
    pub const fn outcome(&self) -> IncrementalInteractionTokenKeyringOutcomeV1 {
        self.outcome
    }

    pub fn active_key_id(&self) -> &str {
        &self.active_key_id
    }
}

enum IncrementalInteractionTokenKeyringPlanV1 {
    Create(KeyringSecretV1),
    ExactReplay(String),
}

pub fn provision_interaction_token_keyring(
    _acknowledgement: StagingAcknowledgementV1,
) -> Result<IncrementalInteractionTokenKeyringReportV1, ProvisionerErrorV1> {
    if postgres_environment_is_present() {
        return Err(ProvisionerErrorV1::PostgresEnvironment);
    }
    let keychain = KeychainClientV1::new()?;
    let product_action = keychain.read_required(PRODUCT_ACTION_KEYRING_IDENTITY)?;
    let snapshot_envelope = keychain.read_required(SNAPSHOT_ENVELOPE_KEYRING_IDENTITY)?;
    let existing = keychain.read_optional(INTERACTION_TOKEN_ENVELOPE_KEYRING_IDENTITY)?;
    let mut random = SystemRandomSourceV1;
    match plan_incremental_interaction_token_keyring(
        &product_action,
        &snapshot_envelope,
        existing.as_ref().map(|value| value.as_slice()),
        &mut random,
    )? {
        IncrementalInteractionTokenKeyringPlanV1::ExactReplay(active_key_id) => {
            Ok(IncrementalInteractionTokenKeyringReportV1 {
                outcome: IncrementalInteractionTokenKeyringOutcomeV1::ExactReplay,
                active_key_id,
            })
        }
        IncrementalInteractionTokenKeyringPlanV1::Create(keyring) => {
            let update = match keychain.begin_create_interaction_token_keyring(SecretItemRefV1 {
                identity: keyring.identity(),
                value: keyring.payload(),
            }) {
                Ok(update) => update,
                Err(
                    ProvisionerErrorV1::IncrementalInteractionTokenKeyringBusy
                    | ProvisionerErrorV1::KeychainWrite,
                ) => {
                    let current = keychain
                        .read_optional(INTERACTION_TOKEN_ENVELOPE_KEYRING_IDENTITY)?
                        .ok_or(ProvisionerErrorV1::IncrementalInteractionTokenKeyringBusy)?;
                    let (_, _, active_key_id) =
                        validate_keyring_set(&product_action, &snapshot_envelope, &current)?;
                    return Ok(IncrementalInteractionTokenKeyringReportV1 {
                        outcome: IncrementalInteractionTokenKeyringOutcomeV1::ExactReplay,
                        active_key_id,
                    });
                }
                Err(error) => return Err(error),
            };
            let active_key_id = keyring.active_key_id().to_owned();
            update.commit();
            Ok(IncrementalInteractionTokenKeyringReportV1 {
                outcome: IncrementalInteractionTokenKeyringOutcomeV1::Created,
                active_key_id,
            })
        }
    }
}

fn plan_incremental_interaction_token_keyring(
    product_action: &[u8],
    snapshot_envelope: &[u8],
    existing: Option<&[u8]>,
    random: &mut impl RandomSourceV1,
) -> Result<IncrementalInteractionTokenKeyringPlanV1, ProvisionerErrorV1> {
    let api_keyrings = validate_api_keyring_pair(product_action, snapshot_envelope)?;
    if let Some(existing) = existing {
        let (_, _, active_key_id) =
            validate_keyring_set(product_action, snapshot_envelope, existing)?;
        return Ok(IncrementalInteractionTokenKeyringPlanV1::ExactReplay(
            active_key_id,
        ));
    }
    let keyring = generate_incremental_interaction_token_envelope_keyring(
        random,
        api_keyrings.product_action_material(),
        api_keyrings.snapshot_envelope_material(),
    )?;
    validate_keyring_set(product_action, snapshot_envelope, keyring.payload())?;
    Ok(IncrementalInteractionTokenKeyringPlanV1::Create(keyring))
}

#[cfg(test)]
mod tests {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;

    use super::*;

    struct ScriptedRandomV1 {
        values: Vec<Vec<u8>>,
    }

    impl RandomSourceV1 for ScriptedRandomV1 {
        fn fill(&mut self, output: &mut [u8]) -> Result<(), ProvisionerErrorV1> {
            if self.values.is_empty() {
                return Err(ProvisionerErrorV1::Random);
            }
            let value = self.values.remove(0);
            if value.len() != output.len() {
                return Err(ProvisionerErrorV1::Random);
            }
            output.copy_from_slice(&value);
            Ok(())
        }
    }

    struct RejectRandomV1;

    impl RandomSourceV1 for RejectRandomV1 {
        fn fill(&mut self, _output: &mut [u8]) -> Result<(), ProvisionerErrorV1> {
            Err(ProvisionerErrorV1::Random)
        }
    }

    fn material(seed: u8) -> [u8; 32] {
        std::array::from_fn(|index| seed.wrapping_add(index as u8))
    }

    fn api_payload(id: &str, material: &[u8; 32]) -> String {
        format!(
            "{{\"version\":1,\"active\":{{\"id\":\"{id}\",\"material\":\"{}\"}},\"retired\":[]}}",
            STANDARD.encode(material)
        )
    }

    fn interaction_payload(id: &str, material: &[u8; 32]) -> String {
        let encoded = material
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        format!("v1;active={id}={encoded};retired=")
    }

    #[test]
    fn missing_item_generates_canonical_material_distinct_from_both_api_keyrings() {
        let product_material = material(1);
        let snapshot_material = material(101);
        let generated_material = material(201);
        let product = api_payload("product", &product_material);
        let snapshot = api_payload("snapshot", &snapshot_material);
        let mut random = ScriptedRandomV1 {
            values: vec![
                (1_u8..=12).collect(),
                product_material.to_vec(),
                generated_material.to_vec(),
            ],
        };
        let plan = plan_incremental_interaction_token_keyring(
            product.as_bytes(),
            snapshot.as_bytes(),
            None,
            &mut random,
        )
        .unwrap();
        let IncrementalInteractionTokenKeyringPlanV1::Create(keyring) = plan else {
            panic!()
        };
        let payload = std::str::from_utf8(keyring.payload()).unwrap();
        assert!(payload.starts_with("v1;active=interaction-token-envelope-v1-"));
        assert!(payload.ends_with(";retired="));
        assert!(payload.contains(
            &generated_material
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        ));
        assert!(random.values.is_empty());
    }

    #[test]
    fn valid_existing_item_is_exact_replay_without_randomness() {
        let product_material = material(1);
        let snapshot_material = material(101);
        let interaction_material = material(201);
        let product = api_payload("product", &product_material);
        let snapshot = api_payload("snapshot", &snapshot_material);
        let existing = interaction_payload("interaction", &interaction_material);
        let plan = plan_incremental_interaction_token_keyring(
            product.as_bytes(),
            snapshot.as_bytes(),
            Some(existing.as_bytes()),
            &mut RejectRandomV1,
        )
        .unwrap();
        let IncrementalInteractionTokenKeyringPlanV1::ExactReplay(active_key_id) = plan else {
            panic!()
        };
        assert_eq!(active_key_id, "interaction");
    }

    #[test]
    fn invalid_api_or_existing_runtime_item_fails_before_randomness() {
        let product_material = material(1);
        let snapshot_material = material(101);
        let product = api_payload("same", &product_material);
        let duplicate_id_snapshot = api_payload("same", &snapshot_material);
        assert!(matches!(
            plan_incremental_interaction_token_keyring(
                product.as_bytes(),
                duplicate_id_snapshot.as_bytes(),
                None,
                &mut RejectRandomV1,
            ),
            Err(ProvisionerErrorV1::KeyringContract)
        ));
        let snapshot = api_payload("snapshot", &snapshot_material);
        assert!(matches!(
            plan_incremental_interaction_token_keyring(
                product.as_bytes(),
                snapshot.as_bytes(),
                Some(b"invalid"),
                &mut RejectRandomV1,
            ),
            Err(ProvisionerErrorV1::KeyringContract)
        ));
        let aliased_existing = interaction_payload("interaction", &product_material);
        assert!(matches!(
            plan_incremental_interaction_token_keyring(
                product.as_bytes(),
                snapshot.as_bytes(),
                Some(aliased_existing.as_bytes()),
                &mut RejectRandomV1,
            ),
            Err(ProvisionerErrorV1::KeyringContract)
        ));
    }

    #[test]
    fn report_debug_surface_contains_only_outcome_and_key_id() {
        let report = IncrementalInteractionTokenKeyringReportV1 {
            outcome: IncrementalInteractionTokenKeyringOutcomeV1::Created,
            active_key_id: "interaction".to_string(),
        };
        assert_eq!(
            format!("{report:?}"),
            "IncrementalInteractionTokenKeyringReportV1 { outcome: Created, active_key_id: \"interaction\" }"
        );
    }
}
