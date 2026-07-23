use std::fmt::{Debug, Formatter};

use chrono::{DateTime, Utc};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RuntimeCapabilityReadinessKindV2 {
    Convergence,
    ExactTarget,
    Panel,
    Serving,
    Interaction,
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeCapabilityReadinessReceiptV2 {
    kind: RuntimeCapabilityReadinessKindV2,
    database_identity: String,
    database_name: String,
    executor_role: String,
    checked_at: DateTime<Utc>,
}

impl RuntimeCapabilityReadinessReceiptV2 {
    pub fn new(
        kind: RuntimeCapabilityReadinessKindV2,
        database_identity: impl Into<String>,
        database_name: impl Into<String>,
        executor_role: impl Into<String>,
        checked_at: DateTime<Utc>,
    ) -> Result<Self, RuntimeCapabilityReadinessErrorV2> {
        let database_identity = database_identity.into();
        let database_name = database_name.into();
        let executor_role = executor_role.into();
        if !canonical_database_identity(&database_identity) {
            return Err(RuntimeCapabilityReadinessErrorV2::InvalidDatabaseIdentity);
        }
        if !valid_database_identifier(&database_name) || !valid_database_identifier(&executor_role)
        {
            return Err(RuntimeCapabilityReadinessErrorV2::InvalidDatabaseIdentifier);
        }
        Ok(Self {
            kind,
            database_identity,
            database_name,
            executor_role,
            checked_at,
        })
    }

    pub fn kind(&self) -> RuntimeCapabilityReadinessKindV2 {
        self.kind
    }

    pub fn checked_at(&self) -> DateTime<Utc> {
        self.checked_at
    }
}

impl Debug for RuntimeCapabilityReadinessReceiptV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeCapabilityReadinessReceiptV2(<redacted>)")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeCapabilityReadinessSetV2 {
    convergence: RuntimeCapabilityReadinessReceiptV2,
    exact_target: RuntimeCapabilityReadinessReceiptV2,
    panel: RuntimeCapabilityReadinessReceiptV2,
    serving: RuntimeCapabilityReadinessReceiptV2,
    interaction: RuntimeCapabilityReadinessReceiptV2,
}

impl RuntimeCapabilityReadinessSetV2 {
    pub fn new(
        convergence: RuntimeCapabilityReadinessReceiptV2,
        exact_target: RuntimeCapabilityReadinessReceiptV2,
        panel: RuntimeCapabilityReadinessReceiptV2,
        serving: RuntimeCapabilityReadinessReceiptV2,
        interaction: RuntimeCapabilityReadinessReceiptV2,
    ) -> Result<Self, RuntimeCapabilityReadinessErrorV2> {
        let receipts = [&convergence, &exact_target, &panel, &serving, &interaction];
        let expected_kinds = [
            RuntimeCapabilityReadinessKindV2::Convergence,
            RuntimeCapabilityReadinessKindV2::ExactTarget,
            RuntimeCapabilityReadinessKindV2::Panel,
            RuntimeCapabilityReadinessKindV2::Serving,
            RuntimeCapabilityReadinessKindV2::Interaction,
        ];
        if receipts
            .iter()
            .zip(expected_kinds)
            .any(|(receipt, expected)| receipt.kind != expected)
        {
            return Err(RuntimeCapabilityReadinessErrorV2::CapabilityMismatch);
        }
        let expected_identity = &receipts[0].database_identity;
        let expected_name = &receipts[0].database_name;
        if receipts.iter().any(|receipt| {
            receipt.database_identity != *expected_identity
                || receipt.database_name != *expected_name
        }) {
            return Err(RuntimeCapabilityReadinessErrorV2::AuthorityMismatch);
        }
        for left in 0..receipts.len() {
            for right in left + 1..receipts.len() {
                if receipts[left].executor_role == receipts[right].executor_role {
                    return Err(RuntimeCapabilityReadinessErrorV2::AuthorityMismatch);
                }
            }
        }
        Ok(Self {
            convergence,
            exact_target,
            panel,
            serving,
            interaction,
        })
    }

    pub fn checked_at_bounds(&self) -> (DateTime<Utc>, DateTime<Utc>) {
        let receipts = self.receipts();
        let oldest = receipts
            .iter()
            .map(|receipt| receipt.checked_at)
            .min()
            .expect("readiness set is nonempty");
        let newest = receipts
            .iter()
            .map(|receipt| receipt.checked_at)
            .max()
            .expect("readiness set is nonempty");
        (oldest, newest)
    }

    pub fn all_checked_at_or_after(&self, cutoff: DateTime<Utc>) -> bool {
        self.receipts()
            .iter()
            .all(|receipt| receipt.checked_at >= cutoff)
    }

    fn receipts(&self) -> [&RuntimeCapabilityReadinessReceiptV2; 5] {
        [
            &self.convergence,
            &self.exact_target,
            &self.panel,
            &self.serving,
            &self.interaction,
        ]
    }
}

impl Debug for RuntimeCapabilityReadinessSetV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeCapabilityReadinessSetV2(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeCapabilityReadinessErrorV2 {
    #[error("runtime capability readiness database identity is invalid")]
    InvalidDatabaseIdentity,
    #[error("runtime capability readiness database identifier is invalid")]
    InvalidDatabaseIdentifier,
    #[error("runtime capability readiness kind does not match its slot")]
    CapabilityMismatch,
    #[error("runtime capability readiness authority does not match")]
    AuthorityMismatch,
}

fn canonical_database_identity(value: &str) -> bool {
    if value.len() != 36 {
        return false;
    }
    value.bytes().enumerate().all(|(index, byte)| match index {
        8 | 13 | 18 | 23 => byte == b'-',
        _ => byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte),
    }) && value != "00000000-0000-0000-0000-000000000000"
}

fn valid_database_identifier(value: &str) -> bool {
    let mut bytes = value.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    value.len() <= 63
        && (first.is_ascii_lowercase() || first == b'_')
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};

    use super::{
        RuntimeCapabilityReadinessErrorV2, RuntimeCapabilityReadinessKindV2,
        RuntimeCapabilityReadinessReceiptV2, RuntimeCapabilityReadinessSetV2,
    };

    const IDENTITY: &str = "01234567-89ab-cdef-8123-456789abcdef";

    fn at(second: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(second, 0).unwrap()
    }

    fn receipt(
        kind: RuntimeCapabilityReadinessKindV2,
        role: &str,
        checked_at: i64,
    ) -> RuntimeCapabilityReadinessReceiptV2 {
        RuntimeCapabilityReadinessReceiptV2::new(kind, IDENTITY, "starring", role, at(checked_at))
            .unwrap()
    }

    fn set() -> RuntimeCapabilityReadinessSetV2 {
        RuntimeCapabilityReadinessSetV2::new(
            receipt(RuntimeCapabilityReadinessKindV2::Convergence, "role_a", 1),
            receipt(RuntimeCapabilityReadinessKindV2::ExactTarget, "role_b", 2),
            receipt(RuntimeCapabilityReadinessKindV2::Panel, "role_c", 3),
            receipt(RuntimeCapabilityReadinessKindV2::Serving, "role_d", 4),
            receipt(RuntimeCapabilityReadinessKindV2::Interaction, "role_e", 5),
        )
        .unwrap()
    }

    #[test]
    fn exact_five_capability_set_preserves_freshness_without_exposing_authority() {
        let readiness = set();

        assert_eq!(readiness.checked_at_bounds(), (at(1), at(5)));
        assert!(readiness.all_checked_at_or_after(at(1)));
        assert!(!readiness.all_checked_at_or_after(at(2)));
        assert_eq!(
            format!("{readiness:?}"),
            "RuntimeCapabilityReadinessSetV2(<redacted>)"
        );
        assert_eq!(
            format!(
                "{:?}",
                receipt(RuntimeCapabilityReadinessKindV2::Convergence, "role_a", 1)
            ),
            "RuntimeCapabilityReadinessReceiptV2(<redacted>)"
        );
    }

    #[test]
    fn capability_slots_and_roles_are_exact() {
        assert_eq!(
            RuntimeCapabilityReadinessSetV2::new(
                receipt(RuntimeCapabilityReadinessKindV2::Panel, "role_a", 1),
                receipt(RuntimeCapabilityReadinessKindV2::ExactTarget, "role_b", 2),
                receipt(RuntimeCapabilityReadinessKindV2::Convergence, "role_c", 3),
                receipt(RuntimeCapabilityReadinessKindV2::Serving, "role_d", 4),
                receipt(RuntimeCapabilityReadinessKindV2::Interaction, "role_e", 5),
            ),
            Err(RuntimeCapabilityReadinessErrorV2::CapabilityMismatch)
        );
        assert_eq!(
            RuntimeCapabilityReadinessSetV2::new(
                receipt(RuntimeCapabilityReadinessKindV2::Convergence, "role_a", 1),
                receipt(RuntimeCapabilityReadinessKindV2::ExactTarget, "role_a", 2),
                receipt(RuntimeCapabilityReadinessKindV2::Panel, "role_c", 3),
                receipt(RuntimeCapabilityReadinessKindV2::Serving, "role_d", 4),
                receipt(RuntimeCapabilityReadinessKindV2::Interaction, "role_e", 5),
            ),
            Err(RuntimeCapabilityReadinessErrorV2::AuthorityMismatch)
        );
        let foreign_identity = RuntimeCapabilityReadinessReceiptV2::new(
            RuntimeCapabilityReadinessKindV2::Interaction,
            "11234567-89ab-cdef-8123-456789abcdef",
            "starring",
            "role_e",
            at(5),
        )
        .unwrap();
        assert_eq!(
            RuntimeCapabilityReadinessSetV2::new(
                receipt(RuntimeCapabilityReadinessKindV2::Convergence, "role_a", 1),
                receipt(RuntimeCapabilityReadinessKindV2::ExactTarget, "role_b", 2),
                receipt(RuntimeCapabilityReadinessKindV2::Panel, "role_c", 3),
                receipt(RuntimeCapabilityReadinessKindV2::Serving, "role_d", 4),
                foreign_identity,
            ),
            Err(RuntimeCapabilityReadinessErrorV2::AuthorityMismatch)
        );
        let foreign_database = RuntimeCapabilityReadinessReceiptV2::new(
            RuntimeCapabilityReadinessKindV2::Interaction,
            IDENTITY,
            "other",
            "role_e",
            at(5),
        )
        .unwrap();
        assert_eq!(
            RuntimeCapabilityReadinessSetV2::new(
                receipt(RuntimeCapabilityReadinessKindV2::Convergence, "role_a", 1),
                receipt(RuntimeCapabilityReadinessKindV2::ExactTarget, "role_b", 2),
                receipt(RuntimeCapabilityReadinessKindV2::Panel, "role_c", 3),
                receipt(RuntimeCapabilityReadinessKindV2::Serving, "role_d", 4),
                foreign_database,
            ),
            Err(RuntimeCapabilityReadinessErrorV2::AuthorityMismatch)
        );
    }

    #[test]
    fn malformed_database_authority_is_rejected() {
        for identity in [
            "00000000-0000-0000-0000-000000000000",
            "01234567-89AB-CDEF-8123-456789ABCDEF",
            "01234567-89ab-cdef-8123-456789abcdeg",
        ] {
            assert_eq!(
                RuntimeCapabilityReadinessReceiptV2::new(
                    RuntimeCapabilityReadinessKindV2::Convergence,
                    identity,
                    "starring",
                    "role_a",
                    at(1),
                ),
                Err(RuntimeCapabilityReadinessErrorV2::InvalidDatabaseIdentity)
            );
        }
        for identifier in ["", "UPPER", "leading-dash", &"a".repeat(64)] {
            assert_eq!(
                RuntimeCapabilityReadinessReceiptV2::new(
                    RuntimeCapabilityReadinessKindV2::Convergence,
                    IDENTITY,
                    identifier,
                    "role_a",
                    at(1),
                ),
                Err(RuntimeCapabilityReadinessErrorV2::InvalidDatabaseIdentifier)
            );
        }
    }
}
