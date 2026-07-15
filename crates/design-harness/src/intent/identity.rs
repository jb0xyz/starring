use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::errors::StructuredError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct IdentityErrorSpec<'a> {
    code: &'a str,
    location: &'a str,
    message: &'a str,
}

impl<'a> IdentityErrorSpec<'a> {
    pub(crate) const fn new(code: &'a str, location: &'a str, message: &'a str) -> Self {
        Self {
            code,
            location,
            message,
        }
    }

    fn structured(self, hint: impl Into<String>) -> StructuredError {
        StructuredError::new(self.code, self.location, self.message, hint)
    }
}

pub(crate) fn canonical_json_digest<T: Serialize + ?Sized>(
    domain: &[u8],
    value: &T,
    error: IdentityErrorSpec<'_>,
) -> Result<String, StructuredError> {
    let value =
        serde_json::to_value(value).map_err(|source| error.structured(source.to_string()))?;
    let canonical = canonicalize_json(value);
    compatibility_json_digest(domain, &canonical, error)
}

pub(crate) fn compatibility_json_digest<T: Serialize + ?Sized>(
    prefix: &[u8],
    value: &T,
    error: IdentityErrorSpec<'_>,
) -> Result<String, StructuredError> {
    let bytes = serde_json::to_vec(value).map_err(|source| error.structured(source.to_string()))?;
    let mut hasher = Sha256::new();
    hasher.update(prefix);
    hasher.update(bytes);
    Ok(lowercase_hex(hasher.finalize()))
}

pub(crate) fn domain_separated_length_framed_digest(domain: &[u8], fields: &[&[u8]]) -> String {
    let mut hasher = Sha256::new();
    update_length_framed(&mut hasher, domain);
    for field in fields {
        update_length_framed(&mut hasher, field);
    }
    lowercase_hex(hasher.finalize())
}

pub(crate) fn is_lowercase_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
}

fn canonicalize_json(value: Value) -> Value {
    match value {
        Value::Object(values) => {
            let mut entries = values.into_iter().collect::<Vec<_>>();
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key, canonicalize_json(value)))
                    .collect(),
            )
        }
        Value::Array(values) => Value::Array(values.into_iter().map(canonicalize_json).collect()),
        value => value,
    }
}

fn update_length_framed(hasher: &mut Sha256, bytes: &[u8]) {
    let length = u64::try_from(bytes.len()).expect("identity field length exceeds u64::MAX");
    hasher.update(length.to_be_bytes());
    hasher.update(bytes);
}

fn lowercase_hex(bytes: impl AsRef<[u8]>) -> String {
    let bytes = bytes.as_ref();
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        write!(&mut output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    output
}

#[cfg(test)]
mod tests {
    use serde::ser::Error as _;
    use serde::{Serialize, Serializer};
    use serde_json::json;

    use super::{
        canonical_json_digest, canonicalize_json, compatibility_json_digest,
        domain_separated_length_framed_digest, is_lowercase_sha256_hex, IdentityErrorSpec,
    };

    const ERROR: IdentityErrorSpec<'static> = IdentityErrorSpec::new(
        "IDENTITY_SERIALIZATION_FAILED",
        "intent.identity",
        "The identity projection could not be serialized",
    );

    #[test]
    fn canonicalization_sorts_nested_objects_and_preserves_array_order() {
        let canonical = canonicalize_json(json!({
            "z": {"b": 2, "a": 1},
            "a": [{"d": 4, "c": 3}, {"b": 2, "a": 1}],
        }));

        assert_eq!(
            serde_json::to_string(&canonical).expect("canonical JSON serializes"),
            r#"{"a":[{"c":3,"d":4},{"a":1,"b":2}],"z":{"a":1,"b":2}}"#
        );
    }

    #[test]
    fn canonical_digest_is_key_order_independent_and_domain_separated() {
        let left =
            serde_json::from_str::<serde_json::Value>(r#"{"outer":{"z":2,"a":1},"list":[2,1]}"#)
                .expect("left JSON parses");
        let right =
            serde_json::from_str::<serde_json::Value>(r#"{"list":[2,1],"outer":{"a":1,"z":2}}"#)
                .expect("right JSON parses");

        let left_digest = canonical_json_digest(b"intent.identity.v1\0", &left, ERROR)
            .expect("left digest succeeds");
        let right_digest = canonical_json_digest(b"intent.identity.v1\0", &right, ERROR)
            .expect("right digest succeeds");
        let other_domain = canonical_json_digest(b"intent.identity.v2\0", &right, ERROR)
            .expect("other domain digest succeeds");

        assert_eq!(left_digest, right_digest);
        assert_eq!(
            left_digest,
            "bb5d2124f940fdc27a56bf43c6d1789a1bd4afd289a8caf99d84dbd51191a173"
        );
        assert_ne!(left_digest, other_domain);
        assert!(is_lowercase_sha256_hex(&left_digest));
    }

    #[test]
    fn canonical_digest_preserves_array_semantics() {
        let forward = canonical_json_digest(b"intent.identity.v1\0", &json!([1, 2]), ERROR)
            .expect("forward digest succeeds");
        let reverse = canonical_json_digest(b"intent.identity.v1\0", &json!([2, 1]), ERROR)
            .expect("reverse digest succeeds");

        assert_ne!(forward, reverse);
    }

    #[test]
    fn compatibility_digest_preserves_existing_serialized_bytes() {
        let digest =
            compatibility_json_digest(b"legacy.identity.v1\0", &json!({"b": 2, "a": 1}), ERROR)
                .expect("compatibility digest succeeds");

        assert_eq!(
            digest,
            "2734f68c1d76ed72d3848ce91f18050cf7aaf454bfb2ab2b78e4a8822743d1ef"
        );
    }

    #[test]
    fn serialization_failure_uses_the_supplied_structured_error_contract() {
        let error = canonical_json_digest(b"intent.identity.v1\0", &SerializationFailure, ERROR)
            .expect_err("serialization must fail");

        assert_eq!(error.code, "IDENTITY_SERIALIZATION_FAILED");
        assert_eq!(error.location, "intent.identity");
        assert_eq!(
            error.message,
            "The identity projection could not be serialized"
        );
        assert!(error.hint.contains("forced serialization failure"));
    }

    #[test]
    fn length_framing_distinguishes_field_boundaries() {
        let joined = domain_separated_length_framed_digest(b"request.v1", &[b"ab", b"c"]);
        let split = domain_separated_length_framed_digest(b"request.v1", &[b"a", b"bc"]);
        let other_domain = domain_separated_length_framed_digest(b"request.v2", &[b"ab", b"c"]);

        assert_ne!(joined, split);
        assert_ne!(joined, other_domain);
        assert_eq!(
            joined,
            "55860bd3ad2431bd1eb12c17b0e245fc1d49bf6b3ea0f7adff7389c11f4943cd"
        );
        assert!(is_lowercase_sha256_hex(&joined));
    }

    #[test]
    fn lowercase_sha256_hex_validation_is_strict() {
        assert!(is_lowercase_sha256_hex(&"0123456789abcdef".repeat(4)));
        assert!(!is_lowercase_sha256_hex(&"0123456789ABCDEF".repeat(4)));
        assert!(!is_lowercase_sha256_hex(&"g".repeat(64)));
        assert!(!is_lowercase_sha256_hex(&"0".repeat(63)));
        assert!(!is_lowercase_sha256_hex(&"0".repeat(65)));
        assert!(!is_lowercase_sha256_hex("가나다"));
    }

    struct SerializationFailure;

    impl Serialize for SerializationFailure {
        fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            Err(S::Error::custom("forced serialization failure"))
        }
    }
}
