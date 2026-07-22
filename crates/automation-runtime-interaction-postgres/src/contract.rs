pub(crate) const DATABASE_READINESS_QUERY: &str =
    "SELECT * FROM public.starring_runtime_interaction_database_readiness_v1()";

pub(crate) const ROUTE_READ_QUERY: &str =
    "SELECT * FROM public.starring_runtime_interaction_route_read_v1($1, $2)";

pub(crate) const PINNED_READ_QUERY: &str =
    "SELECT * FROM public.starring_runtime_interaction_pinned_read_v1($1, $2)";

pub(crate) const INSTANCE_REGISTER_QUERY: &str =
    "SELECT public.starring_runtime_interaction_instance_register_v1(\
        $1, $2, $3, $4, $5, $6, $7) AS outcome";
