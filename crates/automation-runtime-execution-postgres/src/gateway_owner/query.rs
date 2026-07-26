pub(crate) const OBSERVE_GATEWAY_OWNER_QUERY: &str =
    "SELECT * FROM public.starring_runtime_gateway_owner_observe_v1($1)";

pub(crate) const ACQUIRE_GATEWAY_OWNER_QUERY: &str =
    "SELECT * FROM public.starring_runtime_gateway_owner_acquire_v1($1, $2, $3, $4)";

pub(crate) const RENEW_GATEWAY_OWNER_QUERY: &str =
    "SELECT * FROM public.starring_runtime_gateway_owner_renew_v1($1, $2, $3, $4, $5, $6)";

pub(crate) const RELEASE_GATEWAY_OWNER_QUERY: &str =
    "SELECT * FROM public.starring_runtime_gateway_owner_release_v1($1, $2, $3, $4)";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gateway_owner_queries_are_function_only_and_positionally_exact() {
        let cases = [
            (OBSERVE_GATEWAY_OWNER_QUERY, "$1", None),
            (ACQUIRE_GATEWAY_OWNER_QUERY, "$4", Some("$5")),
            (RENEW_GATEWAY_OWNER_QUERY, "$6", Some("$7")),
            (RELEASE_GATEWAY_OWNER_QUERY, "$4", Some("$5")),
        ];
        for (query, last, absent) in cases {
            assert!(query.starts_with("SELECT * FROM public.starring_runtime_gateway_owner_"));
            assert!(query.contains(last));
            if let Some(absent) = absent {
                assert!(!query.contains(absent));
            }
            for forbidden in [
                "INSERT ",
                "UPDATE ",
                "DELETE ",
                "runtime_gateway_owner_slots",
            ] {
                assert!(!query.contains(forbidden));
            }
        }
    }
}
