pub(crate) const DATABASE_READINESS_QUERY: &str =
    "SELECT * FROM public.starring_runtime_interaction_database_readiness_v1()";

pub(crate) const DATABASE_BINDING_QUERY: &str =
    "SELECT public.starring_runtime_interaction_database_identity_v1() \
        AS database_identity, pg_catalog.current_database()::TEXT AS database_name, \
        session_user::TEXT AS executor_role";

pub(crate) const ROUTE_READ_QUERY: &str =
    "SELECT * FROM public.starring_runtime_interaction_route_read_v1($1, $2)";

pub(crate) const PINNED_READ_QUERY: &str =
    "SELECT * FROM public.starring_runtime_interaction_pinned_read_v1($1, $2)";

pub(crate) const INSTANCE_REGISTER_QUERY: &str =
    "SELECT public.starring_runtime_interaction_instance_register_v1(\
        $1, $2, $3, $4, $5, $6, $7) AS outcome";

pub(crate) const INSTANCE_TEARDOWN_GET_QUERY: &str =
    "SELECT * FROM public.starring_runtime_interaction_instance_get_for_teardown_v1($1, $2)";

pub(crate) const INSTANCE_TEARDOWN_CLAIM_QUERY: &str =
    "SELECT public.starring_runtime_interaction_instance_claim_deleting_v1($1, $2) AS outcome";

pub(crate) const INSTANCE_TEARDOWN_MARK_QUERY: &str =
    "SELECT public.starring_runtime_interaction_instance_mark_deleted_v1($1, $2) AS outcome";

pub(crate) const INSTANCE_TEARDOWN_RETRY_QUERY: &str =
    "SELECT * FROM public.starring_runtime_interaction_instance_list_retryable_v1($1, $2)";

pub(crate) const INSTANCE_TEARDOWN_RETRY_SCAN_QUERY: &str =
    "SELECT * FROM public.starring_runtime_interaction_instance_scan_retryable_v2(\
        $1, $2, $3, $4, $5)";
