pub(crate) const DATABASE_READINESS_QUERY: &str =
    "SELECT * FROM public.starring_runtime_serving_database_readiness_v1()";

pub(crate) const DATABASE_READINESS_DEFINITION_QUERY: &str =
    "SELECT pg_catalog.encode(pg_catalog.sha256(pg_catalog.convert_to(\
        pg_catalog.pg_get_functiondef(pg_catalog.to_regprocedure(\
            'public.starring_runtime_serving_database_readiness_v1()'\
        )), 'UTF8')), 'hex')";

pub(crate) const DATABASE_BINDING_QUERY: &str =
    "SELECT public.starring_runtime_serving_database_identity_v1() \
        AS database_identity, pg_catalog.current_database()::TEXT AS database_name, \
        session_user::TEXT AS executor_role";

pub(crate) const HEARTBEAT_QUERY: &str =
    "SELECT * FROM public.starring_runtime_serving_heartbeat_v1(\
        $1, $2, $3, $4, $5, $6, $7, $8, $9)";

pub(crate) const DISCONNECT_QUERY: &str =
    "SELECT * FROM public.starring_runtime_serving_disconnect_v1(\
        $1, $2, $3, $4, $5, $6, $7, $8)";
