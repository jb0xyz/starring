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

impl Debug for ValidatedKeyringV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ValidatedKeyringV1(<redacted>)")
    }
}

pub fn validate_keyring_pair(
    product_action: &[u8],
    snapshot_envelope: &[u8],
) -> Result<(String, String), ProvisionerErrorV1> {
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
    Ok((
        product_action.active_key_id,
        snapshot_envelope.active_key_id,
    ))
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

    #[test]
    fn strict_pair_accepts_only_distinct_v1_active_keys() {
        let product = payload("product-action-v1-a", &[1_u8; 32]);
        let snapshot = payload("snapshot-envelope-v1-b", &[2_u8; 32]);
        assert!(validate_keyring_pair(product.as_bytes(), snapshot.as_bytes()).is_err());
        let product_material = std::array::from_fn::<_, 32, _>(|index| index as u8);
        let snapshot_material =
            std::array::from_fn::<_, 32, _>(|index| 255_u8.wrapping_sub(index as u8));
        let product = payload("product-action-v1-a", &product_material);
        let snapshot = payload("snapshot-envelope-v1-b", &snapshot_material);
        let ids = validate_keyring_pair(product.as_bytes(), snapshot.as_bytes()).unwrap();
        assert_eq!(ids.0, "product-action-v1-a");
        assert_eq!(ids.1, "snapshot-envelope-v1-b");
        let aliased = payload("snapshot-envelope-v1-b", &product_material);
        assert!(validate_keyring_pair(product.as_bytes(), aliased.as_bytes()).is_err());
        let same_id = payload("product-action-v1-a", &snapshot_material);
        assert!(validate_keyring_pair(product.as_bytes(), same_id.as_bytes()).is_err());
    }

    #[test]
    fn strict_pair_rejects_unknown_fields_retired_keys_and_noncanonical_material() {
        let material = std::array::from_fn::<_, 32, _>(|index| index as u8);
        let valid = payload("product-action-v1-a", &material);
        let unknown = valid.replace("\"version\":1", "\"version\":1,\"extra\":true");
        assert!(validate_keyring_pair(unknown.as_bytes(), valid.as_bytes()).is_err());
        let retired = valid.replace(
            "\"retired\":[]",
            "\"retired\":[{\"id\":\"old\",\"material\":\"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=\"}]",
        );
        assert!(validate_keyring_pair(retired.as_bytes(), valid.as_bytes()).is_err());
        let unpadded = valid.replace("=", "");
        assert!(validate_keyring_pair(unpadded.as_bytes(), valid.as_bytes()).is_err());
    }
}
