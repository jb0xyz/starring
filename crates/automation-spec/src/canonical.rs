use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::model::AutomationSpecV1;
use crate::validate::{validate_automation_spec_v1, AutomationSpecValidationErrorV1};

const AUTOMATION_SPEC_DIGEST_DOMAIN_V1: &[u8] = b"starring.automation_spec.v1\0";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AutomationSpecDigestV1([u8; 32]);

impl AutomationSpecDigestV1 {
    pub fn to_hex(self) -> String {
        let mut output = String::with_capacity(64);
        for byte in self.0 {
            use std::fmt::Write;
            write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
        }
        output
    }

    pub fn parse(value: &str) -> Option<Self> {
        if value.len() != 64
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return None;
        }
        let mut bytes = [0u8; 32];
        for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
            let high = (pair[0] as char).to_digit(16)? as u8;
            let low = (pair[1] as char).to_digit(16)? as u8;
            bytes[index] = (high << 4) | low;
        }
        Some(Self(bytes))
    }
}

impl Display for AutomationSpecDigestV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.to_hex())
    }
}

impl Serialize for AutomationSpecDigestV1 {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for AutomationSpecDigestV1 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).ok_or_else(|| {
            serde::de::Error::custom("expected a 64-character lowercase SHA-256 digest")
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AutomationSpecDigestErrorV1 {
    #[error("automation spec is invalid")]
    Invalid(#[from] AutomationSpecValidationErrorV1),
    #[error("automation spec encoding failed")]
    Encoding,
    #[error("automation spec JSON is not canonical")]
    NonCanonical,
}

pub fn canonical_automation_spec_bytes_v1(
    spec: &AutomationSpecV1,
) -> Result<Vec<u8>, AutomationSpecDigestErrorV1> {
    validate_automation_spec_v1(spec)?;
    canonical_json_bytes(spec).map_err(|_| AutomationSpecDigestErrorV1::Encoding)
}

pub fn decode_canonical_automation_spec_v1(
    bytes: &[u8],
) -> Result<AutomationSpecV1, AutomationSpecDigestErrorV1> {
    let spec = serde_json::from_slice::<AutomationSpecV1>(bytes)
        .map_err(|_| AutomationSpecDigestErrorV1::NonCanonical)?;
    let canonical = canonical_automation_spec_bytes_v1(&spec)?;
    if canonical != bytes {
        return Err(AutomationSpecDigestErrorV1::NonCanonical);
    }
    Ok(spec)
}

pub(crate) fn automation_spec_digest_v1(
    spec: &AutomationSpecV1,
) -> Result<AutomationSpecDigestV1, AutomationSpecDigestErrorV1> {
    let bytes = canonical_automation_spec_bytes_v1(spec)?;
    Ok(AutomationSpecDigestV1(framed_sha256(
        AUTOMATION_SPEC_DIGEST_DOMAIN_V1,
        &bytes,
    )))
}

pub(crate) fn canonical_json_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, ()> {
    let value = serde_json::to_value(value).map_err(|_| ())?;
    serde_json::to_vec(&canonicalize(value)).map_err(|_| ())
}

pub(crate) fn framed_sha256(domain: &[u8], bytes: &[u8]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(domain);
    digest.update((bytes.len() as u64).to_be_bytes());
    digest.update(bytes);
    digest.finalize().into()
}

fn canonicalize(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let ordered = map
                .into_iter()
                .map(|(key, value)| (key, canonicalize(value)))
                .collect::<BTreeMap<_, _>>();
            Value::Object(ordered.into_iter().collect())
        }
        Value::Array(items) => Value::Array(items.into_iter().map(canonicalize).collect()),
        value => value,
    }
}
