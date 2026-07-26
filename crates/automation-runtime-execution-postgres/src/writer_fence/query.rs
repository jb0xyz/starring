pub(crate) const OBSERVE_WRITER_FENCE_QUERY: &str =
    "SELECT * FROM public.starring_runtime_writer_fence_observe_v1()";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writer_fence_query_is_function_only_and_argument_free() {
        assert_eq!(
            OBSERVE_WRITER_FENCE_QUERY,
            "SELECT * FROM public.starring_runtime_writer_fence_observe_v1()"
        );
        for forbidden in ["INSERT ", "UPDATE ", "DELETE ", "runtime_writer_fence "] {
            assert!(!OBSERVE_WRITER_FENCE_QUERY.contains(forbidden));
        }
    }
}
