use std::fmt::{Debug, Formatter};

use automation_runtime_controller::RuntimeBuildRevisionV1;

const COMPILED_RUNTIME_BUILD_REVISION: Option<&str> =
    option_env!("STARRING_RUNTIME_BUILD_REVISION");
const GIT_REVISION_BYTES: usize = 40;

#[derive(Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeBuildRevisionBootstrapErrorV1 {
    #[error("compiled runtime build revision is missing")]
    Missing,
    #[error("compiled runtime build revision is invalid")]
    Invalid,
}

impl RuntimeBuildRevisionBootstrapErrorV1 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Missing => "runtime_build_revision_missing",
            Self::Invalid => "runtime_build_revision_invalid",
        }
    }

    pub const fn context(self) -> Option<&'static str> {
        None
    }
}

impl Debug for RuntimeBuildRevisionBootstrapErrorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeBuildRevisionBootstrapErrorV1(<redacted>)")
    }
}

pub(crate) struct CompiledRuntimeBuildRevisionV1 {
    revision: RuntimeBuildRevisionV1,
}

impl Debug for CompiledRuntimeBuildRevisionV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("CompiledRuntimeBuildRevisionV1(<redacted>)")
    }
}

impl CompiledRuntimeBuildRevisionV1 {
    pub(crate) fn into_revision(self) -> RuntimeBuildRevisionV1 {
        self.revision
    }
}

pub(crate) fn bootstrap_compiled_runtime_build_revision_v1(
) -> Result<CompiledRuntimeBuildRevisionV1, RuntimeBuildRevisionBootstrapErrorV1> {
    parse_compiled_runtime_build_revision_v1(COMPILED_RUNTIME_BUILD_REVISION)
}

fn parse_compiled_runtime_build_revision_v1(
    value: Option<&str>,
) -> Result<CompiledRuntimeBuildRevisionV1, RuntimeBuildRevisionBootstrapErrorV1> {
    let value = value.ok_or(RuntimeBuildRevisionBootstrapErrorV1::Missing)?;
    if value.len() != GIT_REVISION_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(RuntimeBuildRevisionBootstrapErrorV1::Invalid);
    }
    let revision = RuntimeBuildRevisionV1::parse(value)
        .map_err(|_| RuntimeBuildRevisionBootstrapErrorV1::Invalid)?;
    Ok(CompiledRuntimeBuildRevisionV1 { revision })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA_40: &str = "0123456789abcdef0123456789abcdef01234567";

    #[test]
    fn exact_full_git_revision_is_accepted_without_normalization() {
        let revision = parse_compiled_runtime_build_revision_v1(Some(SHA_40)).unwrap();

        assert_eq!(revision.into_revision().as_str(), SHA_40);
    }

    #[test]
    fn missing_and_noncanonical_revisions_fail_closed() {
        assert!(matches!(
            parse_compiled_runtime_build_revision_v1(None),
            Err(RuntimeBuildRevisionBootstrapErrorV1::Missing)
        ));
        for value in [
            String::new(),
            "a".repeat(39),
            "a".repeat(41),
            SHA_40.to_ascii_uppercase(),
            format!("{SHA_40}:dirty"),
            format!(" {SHA_40}"),
            format!("{SHA_40}\n"),
            "HEAD".to_owned(),
            "main".to_owned(),
            "unknown".to_owned(),
            "dev".to_owned(),
        ] {
            assert!(matches!(
                parse_compiled_runtime_build_revision_v1(Some(&value)),
                Err(RuntimeBuildRevisionBootstrapErrorV1::Invalid)
            ));
        }
    }

    #[test]
    fn public_values_have_finite_codes_and_redacted_debug() {
        let missing = RuntimeBuildRevisionBootstrapErrorV1::Missing;
        let invalid = RuntimeBuildRevisionBootstrapErrorV1::Invalid;
        let revision = parse_compiled_runtime_build_revision_v1(Some(SHA_40)).unwrap();

        assert_eq!(missing.code(), "runtime_build_revision_missing");
        assert_eq!(invalid.code(), "runtime_build_revision_invalid");
        assert_eq!(missing.context(), None);
        assert_eq!(invalid.context(), None);
        assert_eq!(
            missing.to_string(),
            "compiled runtime build revision is missing"
        );
        assert_eq!(
            invalid.to_string(),
            "compiled runtime build revision is invalid"
        );
        assert!(std::error::Error::source(&missing).is_none());
        assert!(std::error::Error::source(&invalid).is_none());
        assert_eq!(
            format!("{missing:?}"),
            "RuntimeBuildRevisionBootstrapErrorV1(<redacted>)"
        );
        assert_eq!(
            format!("{revision:?}"),
            "CompiledRuntimeBuildRevisionV1(<redacted>)"
        );
    }

    #[test]
    fn configured_compile_time_revision_is_canonical() {
        if COMPILED_RUNTIME_BUILD_REVISION.is_some() {
            bootstrap_compiled_runtime_build_revision_v1().unwrap();
        }
    }
}
