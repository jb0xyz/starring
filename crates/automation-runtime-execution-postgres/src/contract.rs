pub(crate) const DATABASE_READINESS_QUERY: &str =
    "SELECT * FROM public.starring_runtime_execution_database_readiness_v1()";

pub(crate) const DATABASE_READINESS_DEFINITION_QUERY: &str =
    "SELECT pg_catalog.encode(pg_catalog.sha256(pg_catalog.convert_to(\
        pg_catalog.pg_get_functiondef(pg_catalog.to_regprocedure(\
            'public.starring_runtime_execution_database_readiness_v1()'\
        )), 'UTF8')), 'hex')";

pub(crate) const FOUNDATIONAL_CAPABILITY_IDENTITIES_V1: [&str; 2] = [
    "public.starring_runtime_execution_database_readiness_v1()",
    "public.starring_runtime_execution_database_identity_v1()",
];

pub(crate) const OPERATION_CAPABILITY_IDENTITIES_V1: [&str; 13] = [
    "public.starring_runtime_execution_claim_next_v1(text,bigint)",
    "public.starring_runtime_execution_renew_v1(text,text,text,bigint,text,bigint,bigint,bigint,bigint)",
    "public.starring_runtime_execution_mutate_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,jsonb)",
    "public.starring_runtime_execution_certify_prepare_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint)",
    "public.starring_runtime_execution_certify_commit_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint,timestamp with time zone,jsonb,text,jsonb,text)",
    "public.starring_runtime_execution_recover_stale_live_v1()",
    "public.starring_runtime_observe_previous_serving_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,jsonb)",
    "public.starring_runtime_gateway_owner_observe_v1(text)",
    "public.starring_runtime_gateway_owner_acquire_v1(text,text,text,bigint)",
    "public.starring_runtime_gateway_owner_renew_v1(text,text,bigint,text,bigint,bigint)",
    "public.starring_runtime_gateway_owner_release_v1(text,text,bigint,text)",
    "public.starring_runtime_writer_fence_observe_v1()",
    "public.starring_runtime_product_drain_observe_v2(text,text,text,bigint,text,text)",
];

pub(crate) const RUNTIME_EXECUTION_READINESS_DEFINITION_DIGEST_V1: Option<&str> =
    Some("746a7e3ac38bf588665063c9b3d79df4fc18e278e2a4edf4d1e3ea97e723827d");

pub(crate) fn capability_manifest_is_well_formed_v1() -> bool {
    let capabilities = FOUNDATIONAL_CAPABILITY_IDENTITIES_V1
        .iter()
        .chain(OPERATION_CAPABILITY_IDENTITIES_V1.iter());
    if capabilities.clone().count() != 15 {
        return false;
    }
    for (index, capability) in capabilities.clone().enumerate() {
        if !capability.starts_with("public.starring_runtime_")
            || !capability.ends_with(')')
            || capabilities
                .clone()
                .skip(index + 1)
                .any(|other| other == capability)
        {
            return false;
        }
    }
    true
}
