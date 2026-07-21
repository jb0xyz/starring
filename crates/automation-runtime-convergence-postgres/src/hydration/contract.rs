pub(super) const DATABASE_IDENTITY_QUERY: &str =
    "SELECT * FROM public.starring_runtime_exact_target_reader_database_identity_v1()";

pub(super) const EXACT_TARGET_QUERY: &str =
    "SELECT * FROM public.starring_runtime_exact_target_read_v1(\
        $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)";
