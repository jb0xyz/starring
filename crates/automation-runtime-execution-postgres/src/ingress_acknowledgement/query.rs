pub(crate) const OBSERVE_INGRESS_OPEN_ACKNOWLEDGEMENT_QUERY: &str =
    "SELECT * FROM public.starring_runtime_ingress_open_acknowledgement_observe_v2($1)";

pub(crate) const PUBLISH_INGRESS_OPEN_ACKNOWLEDGEMENT_QUERY: &str =
    "SELECT * FROM public.starring_runtime_ingress_open_acknowledgement_publish_v2(\
     $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ingress_acknowledgement_queries_are_function_only_and_positionally_exact() {
        assert!(OBSERVE_INGRESS_OPEN_ACKNOWLEDGEMENT_QUERY.contains("$1"));
        assert!(!OBSERVE_INGRESS_OPEN_ACKNOWLEDGEMENT_QUERY.contains("$2"));
        assert!(PUBLISH_INGRESS_OPEN_ACKNOWLEDGEMENT_QUERY.contains("$17"));
        assert!(!PUBLISH_INGRESS_OPEN_ACKNOWLEDGEMENT_QUERY.contains("$18"));
        for query in [
            OBSERVE_INGRESS_OPEN_ACKNOWLEDGEMENT_QUERY,
            PUBLISH_INGRESS_OPEN_ACKNOWLEDGEMENT_QUERY,
        ] {
            assert!(query.starts_with(
                "SELECT * FROM public.starring_runtime_ingress_open_acknowledgement_"
            ));
            for forbidden in [
                "INSERT ",
                "UPDATE ",
                "DELETE ",
                "runtime_ingress_open_acknowledgements_v2",
            ] {
                assert!(!query.contains(forbidden));
            }
        }
    }
}
