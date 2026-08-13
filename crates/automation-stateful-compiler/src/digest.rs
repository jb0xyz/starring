use std::fmt::{Display, Formatter};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

macro_rules! digest_type {
    ($name:ident, $message:literal) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(pub(crate) [u8; 32]);

        impl $name {
            pub(crate) fn from_bytes(bytes: [u8; 32]) -> Self {
                Self(bytes)
            }

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
                Self::parse(&value).ok_or_else(|| serde::de::Error::custom($message))
            }
        }
    };
}

digest_type!(
    StateDeclarationDigestV1,
    "expected a lowercase state-declaration SHA-256 digest"
);
digest_type!(
    StatefulStateSchemaDigestV1,
    "expected a lowercase compiled-state-schema SHA-256 digest"
);
digest_type!(
    StatefulArtifactDigestV1,
    "expected a lowercase stateful-artifact SHA-256 digest"
);
digest_type!(
    StatefulUnionSourceMapDigestV1,
    "expected a lowercase stateful-source-map SHA-256 digest"
);
digest_type!(
    StatefulCompilationBindingDigestV1,
    "expected a lowercase stateful-compilation-binding SHA-256 digest"
);
digest_type!(
    StatefulBundleDigestV1,
    "expected a lowercase stateful-bundle SHA-256 digest"
);
