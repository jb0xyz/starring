pub(super) const DATABASE_READINESS_QUERY: &str =
    "SELECT * FROM public.starring_runtime_exact_target_database_readiness_v1()";

pub(super) const DATABASE_READINESS_DEFINITION_QUERY: &str =
    "SELECT pg_catalog.encode(pg_catalog.sha256(pg_catalog.convert_to(\
        pg_catalog.pg_get_functiondef(pg_catalog.to_regprocedure(\
            'public.starring_runtime_exact_target_database_readiness_v1()'\
        )), 'UTF8')), 'hex')";

pub(super) const DATABASE_BINDING_QUERY: &str =
    "SELECT public.starring_runtime_exact_target_reader_database_identity_v1() \
        AS database_identity, pg_catalog.current_database()::TEXT AS database_name, \
        session_user::TEXT AS executor_role";

pub(super) const EXACT_TARGET_QUERY: &str =
    "SELECT * FROM public.starring_runtime_exact_target_read_v1(\
        $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)";
