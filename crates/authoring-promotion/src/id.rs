use std::fmt::{Debug, Display, Formatter};
use std::hash::{Hash, Hasher};
use std::num::NonZeroU64;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use zeroize::Zeroizing;

const HASH_LENGTH: usize = 64;
const OPAQUE_ID_MAX_LENGTH: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromotionIdError {
    InvalidLength { actual: usize },
    InvalidCharacter,
}

impl Display for PromotionIdError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLength { actual } => {
                write!(
                    formatter,
                    "hash must contain exactly {HASH_LENGTH} bytes, got {actual}"
                )
            }
            Self::InvalidCharacter => {
                formatter.write_str("hash must contain only lowercase hexadecimal characters")
            }
        }
    }
}

impl std::error::Error for PromotionIdError {}

fn parse_hash(value: &str) -> Result<String, PromotionIdError> {
    if value.len() != HASH_LENGTH {
        return Err(PromotionIdError::InvalidLength {
            actual: value.len(),
        });
    }
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(PromotionIdError::InvalidCharacter);
    }
    Ok(value.to_string())
}

macro_rules! define_hash {
    ($name:ident) => {
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            pub fn parse(value: &str) -> Result<Self, PromotionIdError> {
                parse_hash(value).map(Self)
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Display for $name {
            fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::parse(&value).map_err(serde::de::Error::custom)
            }
        }
    };
}

define_hash!(PromotionId);
define_hash!(IdempotencyScopeDigest);
define_hash!(PromotionRequestDigest);
define_hash!(AuthoringHash);

impl PromotionId {
    pub(crate) fn from_scope_digest(digest: &IdempotencyScopeDigest) -> Self {
        Self(digest.as_str().to_string())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpaqueIdError {
    Empty,
    TooLong { actual: usize },
    InvalidCharacter,
}

impl Display for OpaqueIdError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => formatter.write_str("identifier must not be empty"),
            Self::TooLong { actual } => write!(
                formatter,
                "identifier must contain at most {OPAQUE_ID_MAX_LENGTH} bytes, got {actual}"
            ),
            Self::InvalidCharacter => formatter.write_str(
                "identifier must contain only ASCII alphanumeric characters or '_', '-', '.', ':'",
            ),
        }
    }
}

impl std::error::Error for OpaqueIdError {}

fn validate_opaque_id(value: &str) -> Result<(), OpaqueIdError> {
    if value.is_empty() {
        return Err(OpaqueIdError::Empty);
    }
    if value.len() > OPAQUE_ID_MAX_LENGTH {
        return Err(OpaqueIdError::TooLong {
            actual: value.len(),
        });
    }
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
    {
        return Err(OpaqueIdError::InvalidCharacter);
    }
    Ok(())
}

fn parse_opaque_id(value: &str) -> Result<String, OpaqueIdError> {
    validate_opaque_id(value)?;
    Ok(value.to_string())
}

macro_rules! define_opaque_id {
    ($name:ident) => {
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            pub fn parse(value: &str) -> Result<Self, OpaqueIdError> {
                parse_opaque_id(value).map(Self)
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Display for $name {
            fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::parse(&value).map_err(serde::de::Error::custom)
            }
        }
    };
}

define_opaque_id!(TenantId);
define_opaque_id!(PrincipalId);
define_opaque_id!(AuthoringSessionId);
define_opaque_id!(AutomationInstallationId);

#[derive(Clone, PartialEq, Eq)]
pub struct IdempotencyKey(Zeroizing<String>);

impl IdempotencyKey {
    pub fn parse(value: &str) -> Result<Self, OpaqueIdError> {
        parse_opaque_id(value).map(Zeroizing::new).map(Self)
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn validate_secret(value: &str) -> Result<(), OpaqueIdError> {
        validate_opaque_id(value)
    }
}

impl Debug for IdempotencyKey {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("IdempotencyKey(<redacted>)")
    }
}

impl PartialOrd for IdempotencyKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for IdempotencyKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.as_str().cmp(other.0.as_str())
    }
}

impl Hash for IdempotencyKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.as_str().hash(state);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RevisionError {
    Zero,
    Overflow,
}

impl Display for RevisionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Zero => formatter.write_str("revision must be nonzero"),
            Self::Overflow => formatter.write_str("revision overflow"),
        }
    }
}

impl std::error::Error for RevisionError {}

macro_rules! define_revision {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(NonZeroU64);

        impl $name {
            pub fn new(value: u64) -> Result<Self, RevisionError> {
                NonZeroU64::new(value).map(Self).ok_or(RevisionError::Zero)
            }

            pub fn get(self) -> u64 {
                self.0.get()
            }
        }

        impl Display for $name {
            fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
                write!(formatter, "{}", self.0.get())
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_u64(self.0.get())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = u64::deserialize(deserializer)?;
                Self::new(value).map_err(serde::de::Error::custom)
            }
        }
    };
}

define_revision!(SessionGeneration);
define_revision!(PolicyRevision);
define_revision!(BindingRevision);
define_revision!(PromotionRevision);

impl PromotionRevision {
    pub const FIRST: Self = Self(NonZeroU64::MIN);

    pub fn next(self) -> Result<Self, RevisionError> {
        self.get()
            .checked_add(1)
            .and_then(NonZeroU64::new)
            .map(Self)
            .ok_or(RevisionError::Overflow)
    }
}

#[cfg(test)]
mod tests {
    use std::any::TypeId;

    use super::*;

    fn hash() -> String {
        "ab".repeat(32)
    }

    #[test]
    fn hash_newtypes_are_strict_and_roundtrip() {
        let value = hash();
        let promotion = PromotionId::parse(&value).unwrap();
        let scope = IdempotencyScopeDigest::parse(&value).unwrap();
        let request = PromotionRequestDigest::parse(&value).unwrap();
        let authoring = AuthoringHash::parse(&value).unwrap();

        assert_eq!(promotion.as_str(), value);
        assert_eq!(promotion.to_string(), value);
        assert_eq!(PromotionId::from_scope_digest(&scope), promotion);
        assert_eq!(
            serde_json::to_string(&request).unwrap(),
            format!("\"{value}\"")
        );
        assert_eq!(
            serde_json::from_str::<AuthoringHash>(&format!("\"{value}\""))
                .unwrap()
                .as_str(),
            authoring.as_str()
        );
        assert_ne!(TypeId::of::<PromotionId>(), TypeId::of::<AuthoringHash>());
    }

    #[test]
    fn hash_newtypes_reject_malformed_serde() {
        for malformed in [
            "a".repeat(63),
            "a".repeat(65),
            "AB".repeat(32),
            "gg".repeat(32),
        ] {
            assert!(
                serde_json::from_str::<PromotionRequestDigest>(&format!("\"{malformed}\""))
                    .is_err()
            );
        }
        assert!(serde_json::from_str::<PromotionRequestDigest>("7").is_err());
    }

    #[test]
    fn opaque_ids_accept_only_the_bounded_ascii_alphabet() {
        let value = "tenant_A-1.example:prod";
        let tenant = TenantId::parse(value).unwrap();
        assert_eq!(tenant.as_str(), value);
        assert_eq!(
            serde_json::from_str::<TenantId>(&serde_json::to_string(&tenant).unwrap()).unwrap(),
            tenant
        );
        assert!(PrincipalId::parse("").is_err());
        assert!(AuthoringSessionId::parse(&"a".repeat(129)).is_err());
        assert!(AutomationInstallationId::parse("contains space").is_err());
        assert!(AutomationInstallationId::parse("slash/not-allowed").is_err());
        assert!(AutomationInstallationId::parse("한글").is_err());
        assert!(serde_json::from_str::<TenantId>("42").is_err());
        assert_ne!(TypeId::of::<TenantId>(), TypeId::of::<PrincipalId>());
    }

    #[test]
    fn idempotency_key_debug_is_redacted() {
        let raw = "customer-request-42";
        let key = IdempotencyKey::parse(raw).unwrap();
        let rendered = format!("{key:?}");

        assert_eq!(rendered, "IdempotencyKey(<redacted>)");
        assert!(!rendered.contains(raw));
        assert!(IdempotencyKey::parse("").is_err());
        assert!(IdempotencyKey::parse("not allowed").is_err());
    }

    #[test]
    fn revisions_reject_zero_and_strictly_roundtrip() {
        assert_eq!(SessionGeneration::new(0), Err(RevisionError::Zero));
        assert_eq!(PolicyRevision::new(0), Err(RevisionError::Zero));
        assert_eq!(BindingRevision::new(0), Err(RevisionError::Zero));
        assert_eq!(PromotionRevision::new(0), Err(RevisionError::Zero));
        assert!(serde_json::from_str::<SessionGeneration>("0").is_err());
        assert!(serde_json::from_str::<SessionGeneration>("\"1\"").is_err());
        assert!(serde_json::from_str::<SessionGeneration>("1.0").is_err());
        assert!(serde_json::from_str::<SessionGeneration>("-1").is_err());

        let generation = SessionGeneration::new(7).unwrap();
        assert_eq!(serde_json::to_string(&generation).unwrap(), "7");
        assert_eq!(
            serde_json::from_str::<SessionGeneration>("7").unwrap(),
            generation
        );
    }

    #[test]
    fn promotion_revision_starts_at_one_and_detects_overflow() {
        assert_eq!(PromotionRevision::FIRST.get(), 1);
        assert_eq!(PromotionRevision::FIRST.next().unwrap().get(), 2);
        assert_eq!(
            PromotionRevision::new(u64::MAX).unwrap().next(),
            Err(RevisionError::Overflow)
        );
    }
}
