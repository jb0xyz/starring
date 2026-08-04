use std::collections::BTreeSet;
use std::fmt::{Debug, Formatter};

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::Deserialize;
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

use crate::ProvisionerErrorV1;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EncodedKeyringV1<'a> {
    version: u8,
    #[serde(borrow)]
    active: EncodedKeyV1<'a>,
    #[serde(borrow)]
    retired: Vec<EncodedKeyV1<'a>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EncodedKeyV1<'a> {
    id: &'a str,
    material: &'a str,
}

pub struct ValidatedKeyringV1 {
    active_key_id: String,
    material: Zeroizing<[u8; 32]>,
}

pub struct ValidatedInteractionTokenEnvelopeKeyringV1 {
    active_key_id: String,
    keys: Vec<ValidatedInteractionTokenEnvelopeKeyV1>,
}

pub(crate) struct ValidatedApiKeyringPairV1 {
    product_action: ValidatedKeyringV1,
    snapshot_envelope: ValidatedKeyringV1,
}

struct ValidatedInteractionTokenEnvelopeKeyV1 {
    key_id: String,
    material: Zeroizing<[u8; 32]>,
}

impl Debug for ValidatedKeyringV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ValidatedKeyringV1(<redacted>)")
    }
}

impl Debug for ValidatedInteractionTokenEnvelopeKeyringV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ValidatedInteractionTokenEnvelopeKeyringV1(<redacted>)")
    }
}

pub fn validate_keyring_set(
    product_action: &[u8],
    snapshot_envelope: &[u8],
    interaction_token_envelope: &[u8],
) -> Result<(String, String, String), ProvisionerErrorV1> {
    let api_keyrings = validate_api_keyring_pair(product_action, snapshot_envelope)?;
    let interaction_token_envelope =
        validate_interaction_token_envelope_keyring(interaction_token_envelope)?;
    if interaction_token_envelope.keys.iter().any(|key| {
        key.key_id == api_keyrings.product_action.active_key_id
            || key.key_id == api_keyrings.snapshot_envelope.active_key_id
            || bool::from(
                key.material
                    .as_slice()
                    .ct_eq(api_keyrings.product_action.material.as_slice()),
            )
            || bool::from(
                key.material
                    .as_slice()
                    .ct_eq(api_keyrings.snapshot_envelope.material.as_slice()),
            )
    }) {
        return Err(ProvisionerErrorV1::KeyringContract);
    }
    Ok((
        api_keyrings.product_action.active_key_id,
        api_keyrings.snapshot_envelope.active_key_id,
        interaction_token_envelope.active_key_id,
    ))
}

pub(crate) fn validate_api_keyring_pair(
    product_action: &[u8],
    snapshot_envelope: &[u8],
) -> Result<ValidatedApiKeyringPairV1, ProvisionerErrorV1> {
    let product_action = validate_keyring(product_action)?;
    let snapshot_envelope = validate_keyring(snapshot_envelope)?;
    if product_action.active_key_id == snapshot_envelope.active_key_id
        || bool::from(
            product_action
                .material
                .as_slice()
                .ct_eq(snapshot_envelope.material.as_slice()),
        )
    {
        return Err(ProvisionerErrorV1::KeyringContract);
    }
    Ok(ValidatedApiKeyringPairV1 {
        product_action,
        snapshot_envelope,
    })
}

impl ValidatedApiKeyringPairV1 {
    pub(crate) fn product_action_material(&self) -> &[u8; 32] {
        &self.product_action.material
    }

    pub(crate) fn snapshot_envelope_material(&self) -> &[u8; 32] {
        &self.snapshot_envelope.material
    }
}

pub fn validate_interaction_token_envelope_keyring(
    value: &[u8],
) -> Result<ValidatedInteractionTokenEnvelopeKeyringV1, ProvisionerErrorV1> {
    if value.is_empty() || value.len() > 4096 {
        return Err(ProvisionerErrorV1::KeyringContract);
    }
    let value = std::str::from_utf8(value).map_err(|_| ProvisionerErrorV1::KeyringContract)?;
    let payload = value
        .strip_prefix("v1;active=")
        .ok_or(ProvisionerErrorV1::KeyringContract)?;
    let (active, retired) = payload
        .split_once(";retired=")
        .ok_or(ProvisionerErrorV1::KeyringContract)?;
    if retired.contains(";retired=") {
        return Err(ProvisionerErrorV1::KeyringContract);
    }
    let mut keys = Vec::with_capacity(8);
    keys.push(parse_interaction_token_envelope_key(active)?);
    if !retired.is_empty() {
        for entry in retired.split(',') {
            keys.push(parse_interaction_token_envelope_key(entry)?);
        }
    }
    if keys.len() > 8 {
        return Err(ProvisionerErrorV1::KeyringContract);
    }
    let unique_ids = keys
        .iter()
        .map(|key| key.key_id.as_str())
        .collect::<BTreeSet<_>>();
    if unique_ids.len() != keys.len()
        || keys.iter().enumerate().any(|(index, candidate)| {
            keys.iter().skip(index + 1).any(|other| {
                bool::from(
                    candidate
                        .material
                        .as_slice()
                        .ct_eq(other.material.as_slice()),
                )
            })
        })
    {
        return Err(ProvisionerErrorV1::KeyringContract);
    }
    Ok(ValidatedInteractionTokenEnvelopeKeyringV1 {
        active_key_id: keys[0].key_id.clone(),
        keys,
    })
}

fn parse_interaction_token_envelope_key(
    value: &str,
) -> Result<ValidatedInteractionTokenEnvelopeKeyV1, ProvisionerErrorV1> {
    let (key_id, encoded) = value
        .split_once('=')
        .ok_or(ProvisionerErrorV1::KeyringContract)?;
    if key_id.is_empty()
        || key_id.len() > 128
        || !key_id.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':' | b'/')
        })
        || encoded.len() != 64
        || encoded.contains('=')
    {
        return Err(ProvisionerErrorV1::KeyringContract);
    }
    let mut material = Zeroizing::new([0_u8; 32]);
    for (index, pair) in encoded.as_bytes().chunks_exact(2).enumerate() {
        let high = lower_hex_nibble(pair[0])?;
        let low = lower_hex_nibble(pair[1])?;
        material[index] = (high << 4) | low;
    }
    if [1_usize, 2, 4, 8, 16].into_iter().any(|period| {
        (period..material.len()).all(|index| material[index] == material[index % period])
    }) {
        return Err(ProvisionerErrorV1::KeyringContract);
    }
    Ok(ValidatedInteractionTokenEnvelopeKeyV1 {
        key_id: key_id.to_owned(),
        material,
    })
}

fn lower_hex_nibble(value: u8) -> Result<u8, ProvisionerErrorV1> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        _ => Err(ProvisionerErrorV1::KeyringContract),
    }
}

fn validate_keyring(value: &[u8]) -> Result<ValidatedKeyringV1, ProvisionerErrorV1> {
    if value.is_empty() || value.len() > 4096 {
        return Err(ProvisionerErrorV1::KeyringContract);
    }
    let encoded: EncodedKeyringV1<'_> =
        serde_json::from_slice(value).map_err(|_| ProvisionerErrorV1::KeyringContract)?;
    if encoded.version != 1
        || !encoded.retired.is_empty()
        || encoded.active.id.is_empty()
        || encoded.active.id.len() > 64
        || !encoded
            .active
            .id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
    {
        return Err(ProvisionerErrorV1::KeyringContract);
    }
    let material = encoded.active.material.as_bytes();
    if material.len() != 44
        || material.last() != Some(&b'=')
        || !material[..43]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/'))
    {
        return Err(ProvisionerErrorV1::KeyringContract);
    }
    let mut decoded = Zeroizing::new([0_u8; 33]);
    let decoded_length = STANDARD
        .decode_slice(encoded.active.material, &mut *decoded)
        .map_err(|_| ProvisionerErrorV1::KeyringContract)?;
    if decoded_length != 32
        || decoded[..32]
            .iter()
            .all(|byte| *byte == decoded.first().copied().unwrap_or_default())
    {
        return Err(ProvisionerErrorV1::KeyringContract);
    }
    let mut key_material = Zeroizing::new([0_u8; 32]);
    key_material.copy_from_slice(&decoded[..32]);
    Ok(ValidatedKeyringV1 {
        active_key_id: encoded.active.id.to_owned(),
        material: key_material,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(id: &str, material: &[u8; 32]) -> String {
        format!(
            "{{\"version\":1,\"active\":{{\"id\":\"{id}\",\"material\":\"{}\"}},\"retired\":[]}}",
            STANDARD.encode(material)
        )
    }

    fn interaction_payload(active_id: &str, materials: &[(String, [u8; 32])]) -> String {
        let active = materials
            .first()
            .map(|(_, material)| lower_hex(material))
            .unwrap();
        let retired = materials
            .iter()
            .skip(1)
            .map(|(id, material)| format!("{id}={}", lower_hex(material)))
            .collect::<Vec<_>>()
            .join(",");
        format!("v1;active={active_id}={active};retired={retired}")
    }

    fn lower_hex(material: &[u8; 32]) -> String {
        material.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn material(seed: u8) -> [u8; 32] {
        std::array::from_fn(|index| seed.wrapping_add(index as u8))
    }

    #[test]
    fn strict_pair_accepts_only_distinct_v1_active_keys() {
        let product = payload("product-action-v1-a", &[1_u8; 32]);
        let snapshot = payload("snapshot-envelope-v1-b", &[2_u8; 32]);
        let interaction = interaction_payload(
            "interaction-token-envelope-v1-c",
            &[("interaction-token-envelope-v1-c".to_string(), material(50))],
        );
        assert!(validate_keyring_set(
            product.as_bytes(),
            snapshot.as_bytes(),
            interaction.as_bytes()
        )
        .is_err());
        let product_material = std::array::from_fn::<_, 32, _>(|index| index as u8);
        let snapshot_material =
            std::array::from_fn::<_, 32, _>(|index| 255_u8.wrapping_sub(index as u8));
        let product = payload("product-action-v1-a", &product_material);
        let snapshot = payload("snapshot-envelope-v1-b", &snapshot_material);
        let ids = validate_keyring_set(
            product.as_bytes(),
            snapshot.as_bytes(),
            interaction.as_bytes(),
        )
        .unwrap();
        assert_eq!(ids.0, "product-action-v1-a");
        assert_eq!(ids.1, "snapshot-envelope-v1-b");
        assert_eq!(ids.2, "interaction-token-envelope-v1-c");
        let aliased = payload("snapshot-envelope-v1-b", &product_material);
        assert!(validate_keyring_set(
            product.as_bytes(),
            aliased.as_bytes(),
            interaction.as_bytes()
        )
        .is_err());
        let same_id = payload("product-action-v1-a", &snapshot_material);
        assert!(validate_keyring_set(
            product.as_bytes(),
            same_id.as_bytes(),
            interaction.as_bytes()
        )
        .is_err());
        let cross_alias = interaction_payload(
            "interaction-token-envelope-v1-c",
            &[(
                "interaction-token-envelope-v1-c".to_string(),
                product_material,
            )],
        );
        assert!(validate_keyring_set(
            product.as_bytes(),
            snapshot.as_bytes(),
            cross_alias.as_bytes()
        )
        .is_err());
    }

    #[test]
    fn strict_pair_rejects_unknown_fields_retired_keys_and_noncanonical_material() {
        let api_material = std::array::from_fn::<_, 32, _>(|index| index as u8);
        let valid = payload("product-action-v1-a", &api_material);
        let interaction = interaction_payload(
            "interaction-token-envelope-v1-c",
            &[("interaction-token-envelope-v1-c".to_string(), material(50))],
        );
        let unknown = valid.replace("\"version\":1", "\"version\":1,\"extra\":true");
        assert!(
            validate_keyring_set(unknown.as_bytes(), valid.as_bytes(), interaction.as_bytes())
                .is_err()
        );
        let retired = valid.replace(
            "\"retired\":[]",
            "\"retired\":[{\"id\":\"old\",\"material\":\"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=\"}]",
        );
        assert!(
            validate_keyring_set(retired.as_bytes(), valid.as_bytes(), interaction.as_bytes())
                .is_err()
        );
        let unpadded = valid.replace("=", "");
        assert!(validate_keyring_set(
            unpadded.as_bytes(),
            valid.as_bytes(),
            interaction.as_bytes()
        )
        .is_err());
    }

    #[test]
    fn interaction_keyring_accepts_seven_retired_keys_and_redacts_material() {
        let materials = (0_u8..8)
            .map(|index| (format!("key-{index}"), material(index.wrapping_mul(32))))
            .collect::<Vec<_>>();
        let payload = interaction_payload("key-0", &materials);
        let validated = validate_interaction_token_envelope_keyring(payload.as_bytes()).unwrap();
        assert_eq!(validated.active_key_id, "key-0");
        assert_eq!(validated.keys.len(), 8);
        let rendered = format!("{validated:?}");
        assert_eq!(
            rendered,
            "ValidatedInteractionTokenEnvelopeKeyringV1(<redacted>)"
        );
        assert!(!rendered.contains(&lower_hex(&materials[0].1)));

        let mut too_many = materials;
        too_many.push(("key-8".to_string(), material(240)));
        let payload = interaction_payload("key-0", &too_many);
        assert!(validate_interaction_token_envelope_keyring(payload.as_bytes()).is_err());
    }

    #[test]
    fn interaction_keyring_rejects_noncanonical_duplicate_and_weak_inputs() {
        let first = lower_hex(&material(1));
        let second = lower_hex(&material(101));
        for invalid in [
            format!("active=key-a={first};retired="),
            format!("v1;active=key-a={first}"),
            format!("v1;active=key-a={first};retired=;retired="),
            format!("v1;active=key a={first};retired="),
            format!("v1;active=key-a={};retired=", first.to_uppercase()),
            format!("v1;active=key-a={};retired=", &first[..62]),
            format!("v1;active=key-a={};retired=", "12".repeat(32)),
            format!("v1;active=key-a={first};retired=key-a={second}"),
            format!("v1;active=key-a={first};retired=key-b={first}"),
        ] {
            assert!(validate_interaction_token_envelope_keyring(invalid.as_bytes()).is_err());
        }
    }
}
