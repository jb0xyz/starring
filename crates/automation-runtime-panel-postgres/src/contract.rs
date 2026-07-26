pub(crate) const DATABASE_READINESS_QUERY: &str =
    "SELECT * FROM public.starring_runtime_panel_database_readiness_v1()";

pub(crate) const DATABASE_BINDING_QUERY: &str =
    "SELECT public.starring_runtime_panel_database_identity_v1() \
        AS database_identity, pg_catalog.current_database()::TEXT AS database_name, \
        session_user::TEXT AS executor_role";

pub(crate) const CLAIM_QUERY: &str =
    "SELECT * FROM public.starring_runtime_panel_reconciliation_claim_v1(\
        $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)";

pub(crate) const CHECK_QUERY: &str =
    "SELECT * FROM public.starring_runtime_panel_reconciliation_check_v1(\
        $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19)";

pub(crate) const SNAPSHOT_QUERY: &str =
    "SELECT * FROM public.starring_runtime_panel_reconciliation_snapshot_v1(\
        $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)";

pub(crate) const INSTALLATION_UPSERT_QUERY: &str =
    "SELECT * FROM public.starring_runtime_panel_reconciliation_installation_upsert_v1(\
        $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, \
        $19, $20, $21, $22, $23, $24, $25)";

pub(crate) const INSTALLATION_REMOVE_QUERY: &str =
    "SELECT * FROM public.starring_runtime_panel_reconciliation_installation_remove_v1(\
        $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, \
        $19, $20)";

pub(crate) const JOURNAL_PUT_QUERY: &str =
    "SELECT * FROM public.starring_runtime_panel_reconciliation_journal_put_v1(\
        $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, \
        $19, $20, $21, $22, $23)";

pub(crate) const JOURNAL_REMOVE_QUERY: &str =
    "SELECT * FROM public.starring_runtime_panel_reconciliation_journal_remove_v1(\
        $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, \
        $19, $20)";
