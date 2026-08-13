use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::model::StatefulSpecV1;
use crate::simulate::{
    StatefulSimulationOutcomeV1, StatefulSimulationTraceV1, StatefulSimulationWorkflowKindV1,
    STATEFUL_SIMULATION_TRACE_KIND_V1, STATEFUL_SIMULATION_TRACE_SCHEMA_VERSION_V1,
};
use crate::validate::{
    validate_stateful_spec_v1, StatefulSpecValidationErrorV1, MAX_STATEFUL_SPEC_CANONICAL_BYTES_V1,
};

// A 4,000-byte modal input may consist entirely of JSON-escaped controls (6 encoded bytes each)
// and fan out to 32 parallel writes. The value is repeated in state_after and transition evidence,
// requiring over 1.5 MiB before bounded metadata. Eight MiB safely admits every accepted V1
// fixture while retaining a fixed decoder and identity bound.
pub const MAX_STATEFUL_SIMULATION_TRACE_CANONICAL_BYTES_V1: usize = 8 * 1024 * 1024;

const STATEFUL_SPEC_DIGEST_DOMAIN_V1: &[u8] = b"starring.stateful_spec.v1\0";
const STATEFUL_SIMULATION_TRACE_DIGEST_DOMAIN_V1: &[u8] =
    b"starring.stateful_simulation_trace.v1\0";

macro_rules! digest_type {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub struct $name([u8; 32]);

        impl $name {
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
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
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

        impl Display for $name {
            fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(&self.to_hex())
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(&self.to_hex())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let value = String::deserialize(deserializer)?;
                Self::parse(&value).ok_or_else(|| {
                    serde::de::Error::custom("expected a 64-character lowercase SHA-256 digest")
                })
            }
        }
    };
}

digest_type!(StatefulSpecDigestV1);
digest_type!(StatefulSimulationTraceDigestV1);

#[derive(Debug, thiserror::Error)]
pub enum StatefulSpecDigestErrorV1 {
    #[error("stateful spec is invalid")]
    Invalid(#[from] StatefulSpecValidationErrorV1),
    #[error("stateful spec encoding failed")]
    Encoding,
    #[error("stateful spec JSON is not canonical")]
    NonCanonical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum StatefulSimulationTraceDigestErrorV1 {
    #[error("stateful simulation trace is invalid")]
    InvalidTrace,
    #[error("stateful simulation trace encoding failed")]
    Encoding,
    #[error("stateful simulation trace JSON is not canonical")]
    NonCanonical,
}

pub fn canonical_stateful_spec_bytes_v1(
    spec: &StatefulSpecV1,
) -> Result<Vec<u8>, StatefulSpecDigestErrorV1> {
    validate_stateful_spec_v1(spec)?;
    canonical_json_bytes(spec).map_err(|_| StatefulSpecDigestErrorV1::Encoding)
}

pub fn decode_canonical_stateful_spec_v1(
    bytes: &[u8],
) -> Result<StatefulSpecV1, StatefulSpecDigestErrorV1> {
    if bytes.len() > MAX_STATEFUL_SPEC_CANONICAL_BYTES_V1 {
        return Err(StatefulSpecDigestErrorV1::NonCanonical);
    }
    let spec = serde_json::from_slice::<StatefulSpecV1>(bytes)
        .map_err(|_| StatefulSpecDigestErrorV1::NonCanonical)?;
    let canonical = canonical_stateful_spec_bytes_v1(&spec)?;
    if canonical != bytes {
        return Err(StatefulSpecDigestErrorV1::NonCanonical);
    }
    Ok(spec)
}

pub fn stateful_spec_digest_v1(
    spec: &StatefulSpecV1,
) -> Result<StatefulSpecDigestV1, StatefulSpecDigestErrorV1> {
    let bytes = canonical_stateful_spec_bytes_v1(spec)?;
    Ok(StatefulSpecDigestV1(framed_sha256(
        STATEFUL_SPEC_DIGEST_DOMAIN_V1,
        &bytes,
    )))
}

pub fn canonical_stateful_simulation_trace_bytes_v1(
    trace: &StatefulSimulationTraceV1,
) -> Result<Vec<u8>, StatefulSimulationTraceDigestErrorV1> {
    validate_trace(trace)?;
    let bytes =
        canonical_json_bytes(trace).map_err(|_| StatefulSimulationTraceDigestErrorV1::Encoding)?;
    if bytes.len() > MAX_STATEFUL_SIMULATION_TRACE_CANONICAL_BYTES_V1 {
        return Err(StatefulSimulationTraceDigestErrorV1::InvalidTrace);
    }
    Ok(bytes)
}

pub fn decode_canonical_stateful_simulation_trace_v1(
    bytes: &[u8],
) -> Result<StatefulSimulationTraceV1, StatefulSimulationTraceDigestErrorV1> {
    if bytes.len() > MAX_STATEFUL_SIMULATION_TRACE_CANONICAL_BYTES_V1 {
        return Err(StatefulSimulationTraceDigestErrorV1::NonCanonical);
    }
    let trace = serde_json::from_slice::<StatefulSimulationTraceV1>(bytes)
        .map_err(|_| StatefulSimulationTraceDigestErrorV1::NonCanonical)?;
    let canonical = canonical_stateful_simulation_trace_bytes_v1(&trace)?;
    if canonical != bytes {
        return Err(StatefulSimulationTraceDigestErrorV1::NonCanonical);
    }
    Ok(trace)
}

pub fn stateful_simulation_trace_digest_v1(
    trace: &StatefulSimulationTraceV1,
) -> Result<StatefulSimulationTraceDigestV1, StatefulSimulationTraceDigestErrorV1> {
    let bytes = canonical_stateful_simulation_trace_bytes_v1(trace)?;
    Ok(StatefulSimulationTraceDigestV1(framed_sha256(
        STATEFUL_SIMULATION_TRACE_DIGEST_DOMAIN_V1,
        &bytes,
    )))
}

fn validate_trace(
    trace: &StatefulSimulationTraceV1,
) -> Result<(), StatefulSimulationTraceDigestErrorV1> {
    if trace.schema_version != STATEFUL_SIMULATION_TRACE_SCHEMA_VERSION_V1
        || trace.kind != STATEFUL_SIMULATION_TRACE_KIND_V1
    {
        return Err(StatefulSimulationTraceDigestErrorV1::InvalidTrace);
    }
    let shape_valid = match trace.outcome {
        StatefulSimulationOutcomeV1::NoTriggerMatch => {
            trace.workflow_id.is_none()
                && trace.workflow_kind.is_none()
                && trace.condition_result.is_none()
                && trace.branch.is_none()
                && trace.state_transitions.is_empty()
                && trace.external_node_ids.is_empty()
        }
        StatefulSimulationOutcomeV1::StatelessConditionNotSatisfied => {
            trace.workflow_id.is_some()
                && trace.workflow_kind == Some(StatefulSimulationWorkflowKindV1::Stateless)
                && trace.condition_result == Some(false)
                && trace.branch.is_none()
                && trace.state_transitions.is_empty()
                && trace.external_node_ids.is_empty()
        }
        StatefulSimulationOutcomeV1::StatelessActionsPlanned => {
            trace.workflow_id.is_some()
                && trace.workflow_kind == Some(StatefulSimulationWorkflowKindV1::Stateless)
                && trace.condition_result == Some(true)
                && trace.branch.is_none()
                && trace.state_transitions.is_empty()
                && !trace.external_node_ids.is_empty()
        }
        StatefulSimulationOutcomeV1::StatefulBranchPlanned => {
            trace.workflow_id.is_some()
                && trace.workflow_kind == Some(StatefulSimulationWorkflowKindV1::Stateful)
                && trace.condition_result.is_some()
                && matches!(
                    (trace.condition_result, trace.branch),
                    (
                        Some(true),
                        Some(crate::simulate::StatefulBranchSelectionV1::True)
                    ) | (
                        Some(false),
                        Some(crate::simulate::StatefulBranchSelectionV1::False)
                    )
                )
                && !trace.external_node_ids.is_empty()
        }
    };
    if !shape_valid {
        return Err(StatefulSimulationTraceDigestErrorV1::InvalidTrace);
    }
    if trace.workflow_kind != Some(StatefulSimulationWorkflowKindV1::Stateful)
        && trace.state_before != trace.state_after
    {
        return Err(StatefulSimulationTraceDigestErrorV1::InvalidTrace);
    }
    let mut reconstructed = trace.state_before.clone();
    let mut variables = BTreeSet::new();
    let mut nodes = BTreeSet::new();
    for transition in &trace.state_transitions {
        if !variables.insert(transition.variable_id.as_str())
            || !nodes.insert(transition.node_id.as_str())
            || reconstructed.get(&transition.variable_id) != Some(&transition.before)
        {
            return Err(StatefulSimulationTraceDigestErrorV1::InvalidTrace);
        }
        reconstructed.insert(transition.variable_id.clone(), transition.after.clone());
    }
    if reconstructed != trace.state_after
        || trace
            .external_node_ids
            .iter()
            .collect::<BTreeSet<_>>()
            .len()
            != trace.external_node_ids.len()
    {
        return Err(StatefulSimulationTraceDigestErrorV1::InvalidTrace);
    }
    Ok(())
}

fn canonical_json_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, ()> {
    let value = serde_json::to_value(value).map_err(|_| ())?;
    serde_json::to_vec(&canonicalize(value)).map_err(|_| ())
}

fn framed_sha256(domain: &[u8], bytes: &[u8]) -> [u8; 32] {
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
