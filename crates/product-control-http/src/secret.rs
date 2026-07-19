use std::fmt::{Debug, Formatter};

use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

const SECRET_BYTES: usize = 43;
const OAUTH_CODE_MAX_BYTES: usize = 1_024;
const IDEMPOTENCY_KEY_MAX_BYTES: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SecretParseError {
    #[error("secret has an invalid encoding")]
    InvalidEncoding,
    #[error("OAuth code is invalid")]
    InvalidOAuthCode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("idempotency key is invalid")]
pub struct IdempotencyKeyParseError;

fn parse_base64url_secret(value: &str) -> Result<Zeroizing<String>, SecretParseError> {
    if value.len() == SECRET_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        && value.as_bytes().last().is_some_and(|byte| {
            matches!(
                byte,
                b'A' | b'E'
                    | b'I'
                    | b'M'
                    | b'Q'
                    | b'U'
                    | b'Y'
                    | b'c'
                    | b'g'
                    | b'k'
                    | b'o'
                    | b's'
                    | b'w'
                    | b'0'
                    | b'4'
                    | b'8'
            )
        })
    {
        Ok(Zeroizing::new(value.to_string()))
    } else {
        Err(SecretParseError::InvalidEncoding)
    }
}

macro_rules! define_secret {
    ($name:ident) => {
        pub struct $name(Zeroizing<String>);

        impl $name {
            pub fn parse(value: &str) -> Result<Self, SecretParseError> {
                parse_base64url_secret(value).map(Self)
            }

            pub fn expose_secret(&self) -> &str {
                self.0.as_str()
            }
        }

        impl PartialEq for $name {
            fn eq(&self, other: &Self) -> bool {
                constant_time_secret_eq(self.expose_secret(), other.expose_secret())
            }
        }

        impl Eq for $name {}

        impl Debug for $name {
            fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(concat!(stringify!($name), "(<redacted>)"))
            }
        }
    };
}

define_secret!(SessionCredential);
define_secret!(CsrfSecret);
define_secret!(OAuthState);

pub struct OAuthCode(Zeroizing<String>);

impl OAuthCode {
    pub fn parse(value: &str) -> Result<Self, SecretParseError> {
        if value.is_empty()
            || value.len() > OAUTH_CODE_MAX_BYTES
            || value.chars().any(char::is_control)
        {
            return Err(SecretParseError::InvalidOAuthCode);
        }
        Ok(Self(Zeroizing::new(value.to_string())))
    }

    pub fn expose_secret(&self) -> &str {
        self.0.as_str()
    }
}

impl Debug for OAuthCode {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("OAuthCode(<redacted>)")
    }
}

pub struct IdempotencyKey(Zeroizing<String>);

impl IdempotencyKey {
    pub fn parse(value: &str) -> Result<Self, IdempotencyKeyParseError> {
        if value.is_empty()
            || value.len() > IDEMPOTENCY_KEY_MAX_BYTES
            || !value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':')
            })
        {
            return Err(IdempotencyKeyParseError);
        }
        Ok(Self(Zeroizing::new(value.to_string())))
    }

    pub fn expose_secret(&self) -> &str {
        self.0.as_str()
    }
}

impl PartialEq for IdempotencyKey {
    fn eq(&self, other: &Self) -> bool {
        constant_time_secret_eq(self.expose_secret(), other.expose_secret())
    }
}

impl Eq for IdempotencyKey {}

impl Debug for IdempotencyKey {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("IdempotencyKey(<redacted>)")
    }
}

pub(crate) fn constant_time_secret_eq(left: &str, right: &str) -> bool {
    left.len() == right.len() && left.as_bytes().ct_eq(right.as_bytes()).into()
}
