use std::fmt::{Debug, Formatter};

const SECRET_BYTES: usize = 43;
const OAUTH_CODE_MAX_BYTES: usize = 1_024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SecretParseError {
    #[error("secret has an invalid encoding")]
    InvalidEncoding,
    #[error("OAuth code is invalid")]
    InvalidOAuthCode,
}

fn parse_base64url_secret(value: &str) -> Result<String, SecretParseError> {
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
        Ok(value.to_string())
    } else {
        Err(SecretParseError::InvalidEncoding)
    }
}

macro_rules! define_secret {
    ($name:ident) => {
        #[derive(Clone, PartialEq, Eq)]
        pub struct $name(String);

        impl $name {
            pub fn parse(value: &str) -> Result<Self, SecretParseError> {
                parse_base64url_secret(value).map(Self)
            }

            pub fn expose_secret(&self) -> &str {
                &self.0
            }
        }

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

#[derive(Clone, PartialEq, Eq)]
pub struct OAuthCode(String);

impl OAuthCode {
    pub fn parse(value: &str) -> Result<Self, SecretParseError> {
        if value.is_empty()
            || value.len() > OAUTH_CODE_MAX_BYTES
            || value.chars().any(char::is_control)
        {
            return Err(SecretParseError::InvalidOAuthCode);
        }
        Ok(Self(value.to_string()))
    }

    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl Debug for OAuthCode {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("OAuthCode(<redacted>)")
    }
}
