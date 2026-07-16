pub(super) fn strip_repeated_prefixes<'a>(mut value: &'a str, prefixes: &[&str]) -> &'a str {
    loop {
        let Some(tail) = prefixes
            .iter()
            .find_map(|prefix| value.strip_prefix(prefix))
        else {
            return value;
        };
        value = tail;
    }
}
