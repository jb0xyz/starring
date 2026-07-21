use std::collections::BTreeSet;

use automation_panel_installation::strict::{
    validate_strict_panel_key_v1, StrictPanelActionV1, StrictPanelCleanupKindV1,
    StrictPanelInstallKindV1, StrictPanelReportV1, MAX_STRICT_PANEL_RECORDS_PER_SLOT,
};
use automation_runtime_controller::PanelReportDigestV1;
use automation_runtime_convergence::{
    PanelCertificateId, PanelCertificateV1, ProcessInstanceId, RuntimeDeploymentTargetV1,
    RuntimeGeneration,
};
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};

const REPORT_DOMAIN_V1: &[u8] = b"starring.runtime.panel.report.v1";
const CERTIFICATE_DOMAIN_V1: &[u8] = b"starring.runtime.panel.certificate.v1";
pub const MAX_CERTIFIED_PANEL_OUTCOMES_V1: usize = MAX_STRICT_PANEL_RECORDS_PER_SLOT * 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CertifiedPanelEvidenceErrorV1 {
    #[error("panel report is not eligible")]
    IneligibleReport,
    #[error("panel report exceeds the per-slot record limit")]
    CapacityExceeded,
    #[error("panel report exceeds the certified outcome limit")]
    OutcomeCapacityExceeded,
    #[error("panel report contains too many distinct panel keys")]
    PanelKeyCapacityExceeded,
    #[error("panel report contains an invalid panel key")]
    InvalidPanelKey,
    #[error("panel report contains duplicate terminal panel outcomes")]
    DuplicateTerminalPanelKey,
    #[error("panel report contains a non-terminal eligible action")]
    IneligibleAction,
    #[error("panel report counters do not match its outcomes")]
    CounterMismatch,
    #[error("runtime deployment target has a zero guild identifier")]
    ZeroGuildId,
    #[error("certified panel evidence identity construction failed")]
    IdentityConstruction,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CertifiedPanelEvidenceV1 {
    certificate: PanelCertificateV1,
    report_digest: PanelReportDigestV1,
    report: StrictPanelReportV1,
}

impl CertifiedPanelEvidenceV1 {
    pub fn build(
        target: RuntimeDeploymentTargetV1,
        runtime_generation: RuntimeGeneration,
        process_instance_id: ProcessInstanceId,
        report: StrictPanelReportV1,
        reconciled_at: DateTime<Utc>,
    ) -> Result<Self, CertifiedPanelEvidenceErrorV1> {
        validate_report(&report)?;
        if target.guild_id.0 == 0 {
            return Err(CertifiedPanelEvidenceErrorV1::ZeroGuildId);
        }

        let report_digest_hex = report_digest_hex(&report, REPORT_DOMAIN_V1);
        let report_digest = PanelReportDigestV1::parse(report_digest_hex.clone())
            .map_err(|_| CertifiedPanelEvidenceErrorV1::IdentityConstruction)?;
        let certificate_id = certificate_id(
            &target,
            runtime_generation,
            &process_instance_id,
            &report_digest_hex,
            reconciled_at,
            CERTIFICATE_DOMAIN_V1,
        )?;
        let certificate = PanelCertificateV1 {
            certificate_id,
            target,
            runtime_generation,
            process_instance_id,
            declared_count: report.declared_count,
            installed_count: report.installed_count,
            unchanged_count: report.unchanged_count,
            skipped_transient_count: report.skipped_transient_count,
            skipped_unresolved_channel_count: report.skipped_unresolved_channel_count,
            failed_count: report.failed_count,
            ambiguous_outcome_count: report.ambiguous_outcome_count,
            stale_message_cleanup_pending_count: report.stale_message_cleanup_pending_count,
            orphan_message_cleanup_pending_count: report.orphan_message_cleanup_pending_count,
            reposted_old_message_cleanup_pending_count: report
                .reposted_old_message_cleanup_pending_count,
            reconciled_at,
        };

        Ok(Self {
            certificate,
            report_digest,
            report,
        })
    }

    pub fn certificate(&self) -> &PanelCertificateV1 {
        &self.certificate
    }

    pub fn report_digest(&self) -> &PanelReportDigestV1 {
        &self.report_digest
    }

    pub fn report(&self) -> &StrictPanelReportV1 {
        &self.report
    }

    pub fn into_parts(self) -> (PanelCertificateV1, PanelReportDigestV1, StrictPanelReportV1) {
        (self.certificate, self.report_digest, self.report)
    }
}

fn validate_report(report: &StrictPanelReportV1) -> Result<(), CertifiedPanelEvidenceErrorV1> {
    if usize::try_from(report.declared_count)
        .map_or(true, |count| count > MAX_STRICT_PANEL_RECORDS_PER_SLOT)
    {
        return Err(CertifiedPanelEvidenceErrorV1::CapacityExceeded);
    }
    if report.outcomes.len() > MAX_CERTIFIED_PANEL_OUTCOMES_V1 {
        return Err(CertifiedPanelEvidenceErrorV1::OutcomeCapacityExceeded);
    }
    if !report.is_eligible() {
        return Err(CertifiedPanelEvidenceErrorV1::IneligibleReport);
    }

    let mut panel_keys = BTreeSet::new();
    let mut terminal_panel_keys = BTreeSet::new();
    let mut installed_count = 0u32;
    let mut unchanged_count = 0u32;
    for outcome in &report.outcomes {
        validate_strict_panel_key_v1(&outcome.panel_key)
            .map_err(|_| CertifiedPanelEvidenceErrorV1::InvalidPanelKey)?;
        panel_keys.insert(outcome.panel_key.as_str());
        match &outcome.action {
            StrictPanelActionV1::Installed(_) => {
                if !terminal_panel_keys.insert(outcome.panel_key.as_str()) {
                    return Err(CertifiedPanelEvidenceErrorV1::DuplicateTerminalPanelKey);
                }
                installed_count = installed_count
                    .checked_add(1)
                    .ok_or(CertifiedPanelEvidenceErrorV1::CounterMismatch)?;
            }
            StrictPanelActionV1::Unchanged => {
                if !terminal_panel_keys.insert(outcome.panel_key.as_str()) {
                    return Err(CertifiedPanelEvidenceErrorV1::DuplicateTerminalPanelKey);
                }
                unchanged_count = unchanged_count
                    .checked_add(1)
                    .ok_or(CertifiedPanelEvidenceErrorV1::CounterMismatch)?;
            }
            StrictPanelActionV1::CleanupCompleted(_) => {}
            _ => return Err(CertifiedPanelEvidenceErrorV1::IneligibleAction),
        }
    }
    if panel_keys.len() > MAX_STRICT_PANEL_RECORDS_PER_SLOT {
        return Err(CertifiedPanelEvidenceErrorV1::PanelKeyCapacityExceeded);
    }
    if installed_count != report.installed_count || unchanged_count != report.unchanged_count {
        return Err(CertifiedPanelEvidenceErrorV1::CounterMismatch);
    }
    if terminal_panel_keys.len() != report.declared_count as usize {
        return Err(CertifiedPanelEvidenceErrorV1::CounterMismatch);
    }
    Ok(())
}

fn report_digest_hex(report: &StrictPanelReportV1, domain: &[u8]) -> String {
    let mut digest = FramedSha256V1::new(domain);
    digest.field(b"format_version", &[1]);
    digest.u64(b"outcome_count", report.outcomes.len() as u64);
    for (index, outcome) in report.outcomes.iter().enumerate() {
        digest.u64(b"outcome_index", index as u64);
        digest.field(b"panel_key", outcome.panel_key.as_bytes());
        let (action, kind) = action_identity(&outcome.action);
        digest.field(b"action", action);
        digest.field(b"kind", kind);
    }
    digest.u32(b"declared_count", report.declared_count);
    digest.u32(b"installed_count", report.installed_count);
    digest.u32(b"unchanged_count", report.unchanged_count);
    digest.u32(b"skipped_transient_count", report.skipped_transient_count);
    digest.u32(
        b"skipped_unresolved_channel_count",
        report.skipped_unresolved_channel_count,
    );
    digest.u32(b"failed_count", report.failed_count);
    digest.u32(b"ambiguous_outcome_count", report.ambiguous_outcome_count);
    digest.u32(
        b"stale_message_cleanup_pending_count",
        report.stale_message_cleanup_pending_count,
    );
    digest.u32(
        b"orphan_message_cleanup_pending_count",
        report.orphan_message_cleanup_pending_count,
    );
    digest.u32(
        b"reposted_old_message_cleanup_pending_count",
        report.reposted_old_message_cleanup_pending_count,
    );
    digest.finish()
}

fn certificate_id(
    target: &RuntimeDeploymentTargetV1,
    runtime_generation: RuntimeGeneration,
    process_instance_id: &ProcessInstanceId,
    report_digest: &str,
    reconciled_at: DateTime<Utc>,
    domain: &[u8],
) -> Result<PanelCertificateId, CertifiedPanelEvidenceErrorV1> {
    let mut digest = FramedSha256V1::new(domain);
    digest.field(b"format_version", &[1]);
    digest.u64(b"guild_id", target.guild_id.0);
    digest.field(b"ruleset_key", target.ruleset_key.as_str().as_bytes());
    digest.u32(b"ruleset_version", target.version.get());
    digest.field(b"content_hash", target.content_hash.to_hex().as_bytes());
    digest.u64(b"binding_revision", target.binding_revision.get());
    digest.field(
        b"binding_fingerprint",
        target.binding_fingerprint.as_str().as_bytes(),
    );
    digest.u64(b"runtime_generation", runtime_generation.get());
    digest.field(
        b"process_instance_id",
        process_instance_id.as_str().as_bytes(),
    );
    digest.field(b"report_digest", report_digest.as_bytes());
    digest.i64(b"reconciled_at_seconds", reconciled_at.timestamp());
    digest.u32(
        b"reconciled_at_nanoseconds",
        reconciled_at.timestamp_subsec_nanos(),
    );
    PanelCertificateId::parse(format!("panel:{}", digest.finish()))
        .map_err(|_| CertifiedPanelEvidenceErrorV1::IdentityConstruction)
}

fn action_identity(action: &StrictPanelActionV1) -> (&'static [u8], &'static [u8]) {
    match action {
        StrictPanelActionV1::Installed(kind) => (b"installed", install_kind_identity(*kind)),
        StrictPanelActionV1::Unchanged => (b"unchanged", b""),
        StrictPanelActionV1::CleanupCompleted(kind) => {
            (b"cleanup_completed", cleanup_kind_identity(*kind))
        }
        StrictPanelActionV1::CleanupPending(kind) => {
            (b"cleanup_pending", cleanup_kind_identity(*kind))
        }
        StrictPanelActionV1::PostDefinitelyNotApplied => (b"post_definitely_not_applied", b""),
        StrictPanelActionV1::AmbiguousPost => (b"ambiguous_post", b""),
        StrictPanelActionV1::PostedMessageMissing => (b"posted_message_missing", b""),
        StrictPanelActionV1::PostedPayloadMismatch => (b"posted_payload_mismatch", b""),
        StrictPanelActionV1::SkippedTransient => (b"skipped_transient", b""),
        StrictPanelActionV1::SkippedUnresolvedChannel => (b"skipped_unresolved_channel", b""),
    }
}

fn install_kind_identity(kind: StrictPanelInstallKindV1) -> &'static [u8] {
    match kind {
        StrictPanelInstallKindV1::Fresh => b"fresh",
        StrictPanelInstallKindV1::MissingMessage => b"missing_message",
        StrictPanelInstallKindV1::ChannelMoved => b"channel_moved",
        StrictPanelInstallKindV1::PayloadReplaced => b"payload_replaced",
        StrictPanelInstallKindV1::MetadataUpdated => b"metadata_updated",
    }
}

fn cleanup_kind_identity(kind: StrictPanelCleanupKindV1) -> &'static [u8] {
    match kind {
        StrictPanelCleanupKindV1::Removed => b"removed",
        StrictPanelCleanupKindV1::ChannelMoved => b"channel_moved",
        StrictPanelCleanupKindV1::PayloadReplaced => b"payload_replaced",
        StrictPanelCleanupKindV1::Orphan => b"orphan",
    }
}

struct FramedSha256V1 {
    hasher: Sha256,
}

impl FramedSha256V1 {
    fn new(domain: &[u8]) -> Self {
        let mut value = Self {
            hasher: Sha256::new(),
        };
        value.frame(domain);
        value
    }

    fn field(&mut self, name: &[u8], value: &[u8]) {
        self.frame(name);
        self.frame(value);
    }

    fn u32(&mut self, name: &[u8], value: u32) {
        self.field(name, &value.to_be_bytes());
    }

    fn u64(&mut self, name: &[u8], value: u64) {
        self.field(name, &value.to_be_bytes());
    }

    fn i64(&mut self, name: &[u8], value: i64) {
        self.field(name, &value.to_be_bytes());
    }

    fn frame(&mut self, value: &[u8]) {
        self.hasher.update((value.len() as u64).to_be_bytes());
        self.hasher.update(value);
    }

    fn finish(self) -> String {
        let bytes = self.hasher.finalize();
        let mut output = String::with_capacity(64);
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for byte in bytes {
            output.push(HEX[(byte >> 4) as usize] as char);
            output.push(HEX[(byte & 0x0f) as usize] as char);
        }
        output
    }
}

#[cfg(test)]
mod tests {
    use automation_panel_installation::strict::{
        StrictPanelActionV1, StrictPanelCleanupKindV1, StrictPanelInstallKindV1,
        StrictPanelOutcomeV1, StrictPanelReportV1, MAX_STRICT_PANEL_RECORDS_PER_SLOT,
    };
    use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
    use automation_runtime_convergence::{
        BindingRevision, ProcessInstanceId, RuntimeDeploymentTargetV1, RuntimeGeneration,
    };
    use chrono::{DateTime, Utc};
    use discord_model::GuildId;
    use resource_resolution::ResourceBindingFingerprint;

    use super::{
        certificate_id, report_digest_hex, CertifiedPanelEvidenceErrorV1, CertifiedPanelEvidenceV1,
        CERTIFICATE_DOMAIN_V1, MAX_CERTIFIED_PANEL_OUTCOMES_V1, REPORT_DOMAIN_V1,
    };

    fn target() -> RuntimeDeploymentTargetV1 {
        RuntimeDeploymentTargetV1 {
            guild_id: GuildId(42),
            ruleset_key: RuleSetKey::parse("study_room").unwrap(),
            version: RuleSetVersionId::new(7).unwrap(),
            content_hash: RuleSetContentHash::parse_hex(&"1".repeat(64)).unwrap(),
            binding_revision: BindingRevision::new(11).unwrap(),
            binding_fingerprint: ResourceBindingFingerprint::parse(&"2".repeat(64)).unwrap(),
        }
    }

    fn reconciled_at() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-07-22T08:09:10.123456789Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn report() -> StrictPanelReportV1 {
        StrictPanelReportV1 {
            outcomes: vec![
                StrictPanelOutcomeV1 {
                    panel_key: "create_room".to_string(),
                    action: StrictPanelActionV1::Installed(StrictPanelInstallKindV1::Fresh),
                },
                StrictPanelOutcomeV1 {
                    panel_key: "join_room".to_string(),
                    action: StrictPanelActionV1::Unchanged,
                },
            ],
            declared_count: 2,
            installed_count: 1,
            unchanged_count: 1,
            ..StrictPanelReportV1::default()
        }
    }

    fn build(
        report: StrictPanelReportV1,
    ) -> Result<CertifiedPanelEvidenceV1, CertifiedPanelEvidenceErrorV1> {
        CertifiedPanelEvidenceV1::build(
            target(),
            RuntimeGeneration::new(13).unwrap(),
            ProcessInstanceId::parse("runtime-a").unwrap(),
            report,
            reconciled_at(),
        )
    }

    #[test]
    fn same_input_is_deterministic_and_matches_golden_identities() {
        let first = build(report()).unwrap();
        let second = build(report()).unwrap();
        assert_eq!(first, second);
        assert_eq!(
            first.report_digest().as_str(),
            "447c1d7aec979e4ceb5d3b3f7f019e94e0204ab311b54c41e2f8e0d9065ab676"
        );
        assert_eq!(
            first.certificate().certificate_id.as_str(),
            "panel:39da5c1cdf57c4ec1232575512f5cc6d9c19fa6673603dc4130f4017eae9941e"
        );
        assert!(first
            .certificate()
            .certificate_id
            .as_str()
            .starts_with("panel:"));
        assert_eq!(first.certificate().certificate_id.as_str().len(), 70);
        assert!(first.certificate().certificate_id.as_str()[6..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
    }

    #[test]
    fn report_digest_binds_order_domain_actions_kinds_and_every_counter() {
        let base = report();
        let base_digest = report_digest_hex(&base, REPORT_DOMAIN_V1);

        let mut reordered = base.clone();
        reordered.outcomes.swap(0, 1);
        assert_ne!(base_digest, report_digest_hex(&reordered, REPORT_DOMAIN_V1));
        assert_ne!(
            base_digest,
            report_digest_hex(&base, b"starring.runtime.panel.report.changed")
        );

        let actions = vec![
            StrictPanelActionV1::Installed(StrictPanelInstallKindV1::Fresh),
            StrictPanelActionV1::Installed(StrictPanelInstallKindV1::MissingMessage),
            StrictPanelActionV1::Installed(StrictPanelInstallKindV1::ChannelMoved),
            StrictPanelActionV1::Installed(StrictPanelInstallKindV1::PayloadReplaced),
            StrictPanelActionV1::Installed(StrictPanelInstallKindV1::MetadataUpdated),
            StrictPanelActionV1::Unchanged,
            StrictPanelActionV1::CleanupCompleted(StrictPanelCleanupKindV1::Removed),
            StrictPanelActionV1::CleanupCompleted(StrictPanelCleanupKindV1::ChannelMoved),
            StrictPanelActionV1::CleanupCompleted(StrictPanelCleanupKindV1::PayloadReplaced),
            StrictPanelActionV1::CleanupCompleted(StrictPanelCleanupKindV1::Orphan),
            StrictPanelActionV1::CleanupPending(StrictPanelCleanupKindV1::Removed),
            StrictPanelActionV1::CleanupPending(StrictPanelCleanupKindV1::ChannelMoved),
            StrictPanelActionV1::CleanupPending(StrictPanelCleanupKindV1::PayloadReplaced),
            StrictPanelActionV1::CleanupPending(StrictPanelCleanupKindV1::Orphan),
            StrictPanelActionV1::PostDefinitelyNotApplied,
            StrictPanelActionV1::AmbiguousPost,
            StrictPanelActionV1::PostedMessageMissing,
            StrictPanelActionV1::PostedPayloadMismatch,
            StrictPanelActionV1::SkippedTransient,
            StrictPanelActionV1::SkippedUnresolvedChannel,
        ];
        let action_digests = actions
            .into_iter()
            .map(|action| {
                let mut candidate = StrictPanelReportV1::default();
                candidate.outcomes.push(StrictPanelOutcomeV1 {
                    panel_key: "panel".to_string(),
                    action,
                });
                report_digest_hex(&candidate, REPORT_DOMAIN_V1)
            })
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(action_digests.len(), 20);

        for counter in 0..10 {
            let mut candidate = StrictPanelReportV1::default();
            match counter {
                0 => candidate.declared_count = 1,
                1 => candidate.installed_count = 1,
                2 => candidate.unchanged_count = 1,
                3 => candidate.skipped_transient_count = 1,
                4 => candidate.skipped_unresolved_channel_count = 1,
                5 => candidate.failed_count = 1,
                6 => candidate.ambiguous_outcome_count = 1,
                7 => candidate.stale_message_cleanup_pending_count = 1,
                8 => candidate.orphan_message_cleanup_pending_count = 1,
                9 => candidate.reposted_old_message_cleanup_pending_count = 1,
                _ => unreachable!(),
            }
            assert_ne!(
                report_digest_hex(&StrictPanelReportV1::default(), REPORT_DOMAIN_V1),
                report_digest_hex(&candidate, REPORT_DOMAIN_V1)
            );
        }
    }

    #[test]
    fn certificate_identity_binds_every_scope_field_and_timestamp() {
        let baseline_target = target();
        let generation = RuntimeGeneration::new(13).unwrap();
        let process = ProcessInstanceId::parse("runtime-a").unwrap();
        let digest = "3".repeat(64);
        let time = reconciled_at();
        let baseline = certificate_id(
            &baseline_target,
            generation,
            &process,
            &digest,
            time,
            CERTIFICATE_DOMAIN_V1,
        )
        .unwrap();

        let mut targets = Vec::new();
        let mut changed = target();
        changed.guild_id = GuildId(43);
        targets.push(changed);
        let mut changed = target();
        changed.ruleset_key = RuleSetKey::parse("study_room_2").unwrap();
        targets.push(changed);
        let mut changed = target();
        changed.version = RuleSetVersionId::new(8).unwrap();
        targets.push(changed);
        let mut changed = target();
        changed.content_hash = RuleSetContentHash::parse_hex(&"4".repeat(64)).unwrap();
        targets.push(changed);
        let mut changed = target();
        changed.binding_revision = BindingRevision::new(12).unwrap();
        targets.push(changed);
        let mut changed = target();
        changed.binding_fingerprint = ResourceBindingFingerprint::parse(&"5".repeat(64)).unwrap();
        targets.push(changed);
        for changed in targets {
            let identity = certificate_id(
                &changed,
                generation,
                &process,
                &digest,
                time,
                CERTIFICATE_DOMAIN_V1,
            )
            .unwrap();
            assert_ne!(baseline, identity);
        }

        let changed_generation = certificate_id(
            &baseline_target,
            RuntimeGeneration::new(14).unwrap(),
            &process,
            &digest,
            time,
            CERTIFICATE_DOMAIN_V1,
        )
        .unwrap();
        let changed_process = certificate_id(
            &baseline_target,
            generation,
            &ProcessInstanceId::parse("runtime-b").unwrap(),
            &digest,
            time,
            CERTIFICATE_DOMAIN_V1,
        )
        .unwrap();
        let changed_digest = certificate_id(
            &baseline_target,
            generation,
            &process,
            &"6".repeat(64),
            time,
            CERTIFICATE_DOMAIN_V1,
        )
        .unwrap();
        let changed_time = certificate_id(
            &baseline_target,
            generation,
            &process,
            &digest,
            time + chrono::TimeDelta::nanoseconds(1),
            CERTIFICATE_DOMAIN_V1,
        )
        .unwrap();
        let changed_domain = certificate_id(
            &baseline_target,
            generation,
            &process,
            &digest,
            time,
            b"starring.runtime.panel.certificate.changed",
        )
        .unwrap();
        for changed in [
            changed_generation,
            changed_process,
            changed_digest,
            changed_time,
            changed_domain,
        ] {
            assert_ne!(baseline, changed);
        }
    }

    #[test]
    fn forged_reports_are_rejected() {
        let duplicate = StrictPanelReportV1 {
            outcomes: vec![
                StrictPanelOutcomeV1 {
                    panel_key: "create_room".to_string(),
                    action: StrictPanelActionV1::Installed(StrictPanelInstallKindV1::Fresh),
                },
                StrictPanelOutcomeV1 {
                    panel_key: "create_room".to_string(),
                    action: StrictPanelActionV1::Installed(
                        StrictPanelInstallKindV1::MetadataUpdated,
                    ),
                },
            ],
            declared_count: 2,
            installed_count: 2,
            ..StrictPanelReportV1::default()
        };
        assert_eq!(
            build(duplicate),
            Err(CertifiedPanelEvidenceErrorV1::DuplicateTerminalPanelKey)
        );

        let mut invalid_key = report();
        invalid_key.outcomes[0].panel_key.clear();
        assert_eq!(
            build(invalid_key),
            Err(CertifiedPanelEvidenceErrorV1::InvalidPanelKey)
        );

        let mut wrong_terminal_counters = report();
        wrong_terminal_counters.installed_count = 2;
        wrong_terminal_counters.unchanged_count = 0;
        assert_eq!(
            build(wrong_terminal_counters),
            Err(CertifiedPanelEvidenceErrorV1::CounterMismatch)
        );

        let mut ineligible_action = report();
        ineligible_action.outcomes[0].action = StrictPanelActionV1::AmbiguousPost;
        assert_eq!(
            build(ineligible_action),
            Err(CertifiedPanelEvidenceErrorV1::IneligibleAction)
        );

        let mut ineligible_counter = report();
        ineligible_counter.failed_count = 1;
        assert_eq!(
            build(ineligible_counter),
            Err(CertifiedPanelEvidenceErrorV1::IneligibleReport)
        );

        let outcomes = (0..=MAX_STRICT_PANEL_RECORDS_PER_SLOT)
            .map(|index| StrictPanelOutcomeV1 {
                panel_key: format!("panel_{index}"),
                action: StrictPanelActionV1::CleanupCompleted(StrictPanelCleanupKindV1::Removed),
            })
            .collect();
        let too_many_panel_keys = StrictPanelReportV1 {
            outcomes,
            ..StrictPanelReportV1::default()
        };
        assert_eq!(
            build(too_many_panel_keys),
            Err(CertifiedPanelEvidenceErrorV1::PanelKeyCapacityExceeded)
        );

        let too_many_outcomes = StrictPanelReportV1 {
            outcomes: (0..=MAX_CERTIFIED_PANEL_OUTCOMES_V1)
                .map(|_| StrictPanelOutcomeV1 {
                    panel_key: "panel".to_string(),
                    action: StrictPanelActionV1::CleanupCompleted(
                        StrictPanelCleanupKindV1::Removed,
                    ),
                })
                .collect(),
            ..StrictPanelReportV1::default()
        };
        assert_eq!(
            build(too_many_outcomes),
            Err(CertifiedPanelEvidenceErrorV1::OutcomeCapacityExceeded)
        );

        let too_many_declared = StrictPanelReportV1 {
            declared_count: 257,
            installed_count: 257,
            ..StrictPanelReportV1::default()
        };
        assert_eq!(
            build(too_many_declared),
            Err(CertifiedPanelEvidenceErrorV1::CapacityExceeded)
        );
    }

    #[test]
    fn successful_cleanup_outcomes_can_coexist_with_terminal_outcomes() {
        let report = StrictPanelReportV1 {
            outcomes: vec![
                StrictPanelOutcomeV1 {
                    panel_key: "create_room".to_string(),
                    action: StrictPanelActionV1::CleanupCompleted(StrictPanelCleanupKindV1::Orphan),
                },
                StrictPanelOutcomeV1 {
                    panel_key: "create_room".to_string(),
                    action: StrictPanelActionV1::CleanupCompleted(
                        StrictPanelCleanupKindV1::PayloadReplaced,
                    ),
                },
                StrictPanelOutcomeV1 {
                    panel_key: "create_room".to_string(),
                    action: StrictPanelActionV1::Installed(
                        StrictPanelInstallKindV1::PayloadReplaced,
                    ),
                },
                StrictPanelOutcomeV1 {
                    panel_key: "join_room".to_string(),
                    action: StrictPanelActionV1::Unchanged,
                },
            ],
            declared_count: 2,
            installed_count: 1,
            unchanged_count: 1,
            ..StrictPanelReportV1::default()
        };
        let evidence = build(report.clone()).unwrap();
        assert_eq!(evidence.report(), &report);
        assert_eq!(evidence.certificate().declared_count, 2);
        assert_eq!(evidence.certificate().installed_count, 1);
        assert_eq!(evidence.certificate().unchanged_count, 1);
    }

    #[test]
    fn zero_guild_target_is_rejected() {
        let result = CertifiedPanelEvidenceV1::build(
            RuntimeDeploymentTargetV1 {
                guild_id: GuildId(0),
                ..target()
            },
            RuntimeGeneration::new(13).unwrap(),
            ProcessInstanceId::parse("runtime-a").unwrap(),
            report(),
            reconciled_at(),
        );
        assert_eq!(result, Err(CertifiedPanelEvidenceErrorV1::ZeroGuildId));
    }

    #[test]
    fn certificate_counters_and_owned_report_are_exact() {
        let evidence = build(report()).unwrap();
        let certificate = evidence.certificate();
        assert_eq!(certificate.declared_count, 2);
        assert_eq!(certificate.installed_count, 1);
        assert_eq!(certificate.unchanged_count, 1);
        assert_eq!(certificate.skipped_transient_count, 0);
        assert_eq!(certificate.skipped_unresolved_channel_count, 0);
        assert_eq!(certificate.failed_count, 0);
        assert_eq!(certificate.ambiguous_outcome_count, 0);
        assert_eq!(certificate.stale_message_cleanup_pending_count, 0);
        assert_eq!(certificate.orphan_message_cleanup_pending_count, 0);
        assert_eq!(certificate.reposted_old_message_cleanup_pending_count, 0);
        assert_eq!(evidence.report(), &report());
        let (certificate, digest, owned_report) = evidence.into_parts();
        assert_eq!(certificate.declared_count, 2);
        assert_eq!(digest.as_str().len(), 64);
        assert_eq!(owned_report, report());
    }
}
