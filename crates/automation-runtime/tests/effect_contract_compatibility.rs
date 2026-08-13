use automation_runtime::{
    InteractionActionPreflightCertificateV1 as LegacyCertificateV1,
    InteractionEffectJournalPlanEntryV1 as LegacyJournalEntryV1,
    InteractionEffectJournalPlanV1 as LegacyJournalPlanV1,
};
use automation_runtime_effect_contract::{
    InteractionActionPreflightCertificateV1 as SharedCertificateV1,
    InteractionEffectJournalPlanEntryV1 as SharedJournalEntryV1,
    InteractionEffectJournalPlanV1 as SharedJournalPlanV1,
};

fn shared_certificate_v1(value: LegacyCertificateV1) -> SharedCertificateV1 {
    value
}

fn legacy_certificate_v1(value: SharedCertificateV1) -> LegacyCertificateV1 {
    value
}

fn shared_journal_entry_v1(value: LegacyJournalEntryV1) -> SharedJournalEntryV1 {
    value
}

fn legacy_journal_entry_v1(value: SharedJournalEntryV1) -> LegacyJournalEntryV1 {
    value
}

fn shared_journal_plan_v1(value: LegacyJournalPlanV1) -> SharedJournalPlanV1 {
    value
}

fn legacy_journal_plan_v1(value: SharedJournalPlanV1) -> LegacyJournalPlanV1 {
    value
}

#[test]
fn legacy_runtime_names_are_exact_shared_contract_reexports() {
    let _ = shared_certificate_v1 as fn(LegacyCertificateV1) -> SharedCertificateV1;
    let _ = legacy_certificate_v1 as fn(SharedCertificateV1) -> LegacyCertificateV1;
    let _ = shared_journal_entry_v1 as fn(LegacyJournalEntryV1) -> SharedJournalEntryV1;
    let _ = legacy_journal_entry_v1 as fn(SharedJournalEntryV1) -> LegacyJournalEntryV1;
    let _ = shared_journal_plan_v1 as fn(LegacyJournalPlanV1) -> SharedJournalPlanV1;
    let _ = legacy_journal_plan_v1 as fn(SharedJournalPlanV1) -> LegacyJournalPlanV1;
}
