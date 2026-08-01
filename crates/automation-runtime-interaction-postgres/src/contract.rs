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

pub(crate) const RECEIPT_AUTHORITY_OBSERVE_QUERY: &str =
    "SELECT * FROM public.starring_runtime_interaction_receipt_authority_observe_v1(\
        $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)";

pub(crate) const RECEIPT_CLAIM_QUERY: &str =
    "SELECT * FROM public.starring_runtime_interaction_receipt_claim_v1(\
        $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, \
        $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28, $29, $30, $31, \
        $32, $33, $34, $35, $36, $37, $38, $39, $40, $41)";

pub(crate) const RECEIPT_PLAN_BIND_QUERY: &str =
    "SELECT * FROM public.starring_runtime_interaction_receipt_plan_bind_v1(\
        $1, $2, $3, $4, $5, $6)";

pub(crate) const RECEIPT_ACKNOWLEDGEMENT_INTEND_QUERY: &str =
    "SELECT * FROM public.starring_runtime_interaction_receipt_acknowledgement_intend_v1(\
        $1, $2, $3, $4, $5, $6, $7)";

pub(crate) const RECEIPT_ACKNOWLEDGEMENT_FINISH_QUERY: &str =
    "SELECT * FROM public.starring_runtime_interaction_receipt_acknowledgement_finish_v1(\
        $1, $2, $3, $4, $5, $6, $7, $8)";

pub(crate) const RECEIPT_EXECUTION_INTEND_QUERY: &str =
    "SELECT * FROM public.starring_runtime_interaction_receipt_execution_intend_v1(\
        $1, $2, $3, $4, $5, $6)";

pub(crate) const RECEIPT_FINISH_QUERY: &str =
    "SELECT * FROM public.starring_runtime_interaction_receipt_finish_v1(\
        $1, $2, $3, $4, $5, $6, $7, $8, $9)";

pub(crate) const RECEIPT_RECOVERY_SCAN_QUERY: &str =
    "SELECT * FROM public.starring_runtime_interaction_receipt_scan_recoverable_v1(\
        $1, $2, $3, $4, $5, $6, $7)";

pub(crate) const RECEIPT_RECOVER_QUERY: &str =
    "SELECT * FROM public.starring_runtime_interaction_receipt_recover_v1(\
        $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)";

pub(crate) const RECEIPT_TOKEN_EXPIRE_QUERY: &str =
    "SELECT * FROM public.starring_runtime_interaction_receipt_token_expire_v1(\
        $1, $2, $3, $4, $5)";

pub(crate) const RECEIPT_TERMINALIZE_EXPIRED_QUERY: &str =
    "SELECT * FROM public.starring_runtime_interaction_receipt_terminalize_expired_v1(\
        $1, $2, $3, $4, $5, $6, $7)";

pub(crate) const EFFECT_PLAN_BIND_QUERY: &str =
    "SELECT * FROM public.starring_runtime_interaction_effect_plan_bind_v1(\
        $1, $2, $3, $4, $5, $6, $7, $8, $9)";

pub(crate) const EFFECT_INTEND_QUERY: &str =
    "SELECT * FROM public.starring_runtime_interaction_effect_intend_v1(\
        $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)";

pub(crate) const EFFECT_FINISH_QUERY: &str =
    "SELECT * FROM public.starring_runtime_interaction_effect_finish_v1(\
        $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)";

pub(crate) const EFFECT_RECOVERY_SCAN_QUERY: &str =
    "SELECT * FROM public.starring_runtime_interaction_effect_scan_recoverable_v1(\
        $1, $2, $3, $4, $5, $6, $7, $8, $9)";

pub(crate) const EFFECT_RECOVERY_CLAIM_QUERY: &str =
    "SELECT * FROM public.starring_runtime_interaction_effect_recovery_claim_v1(\
        $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)";

pub(crate) const EFFECT_RESPONSE_TAIL_SCAN_QUERY: &str =
    "SELECT * FROM public.starring_runtime_interaction_effect_response_tail_scan_v1(\
        $1, $2, $3, $4, $5, $6, $7, $8, $9)";

pub(crate) const EFFECT_RESPONSE_TAIL_CLAIM_QUERY: &str =
    "SELECT * FROM public.starring_runtime_interaction_effect_response_tail_claim_v1(\
        $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)";

pub(crate) const EFFECT_RESPONSE_TAIL_FINALIZE_QUERY: &str =
    "SELECT * FROM public.starring_runtime_interaction_effect_response_tail_finalize_v1(\
        $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, \
        $16, $17, $18, $19)";

pub(crate) const EFFECT_RECONCILE_QUERY: &str =
    "SELECT * FROM public.starring_runtime_interaction_effect_reconcile_v1(\
        $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, \
        $17, $18)";

pub(crate) const EFFECT_COMPENSATION_INTEND_QUERY: &str =
    "SELECT * FROM public.starring_runtime_interaction_effect_compensation_intend_v1(\
        $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)";

pub(crate) const EFFECT_COMPENSATION_FINISH_QUERY: &str =
    "SELECT * FROM public.starring_runtime_interaction_effect_compensation_finish_v1(\
        $1, $2, $3, $4, $5, $6, $7, $8, $9, $10)";
