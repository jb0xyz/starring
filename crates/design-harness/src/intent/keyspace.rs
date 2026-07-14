use std::collections::BTreeMap;

pub(super) struct IntentKeyspace {
    feature_id: String,
}

impl IntentKeyspace {
    pub(super) fn new(feature_id: &str) -> Self {
        Self {
            feature_id: feature_id.to_string(),
        }
    }

    pub(super) fn symbol(&self, local_symbol: &str) -> String {
        format!("{}__{local_symbol}", self.feature_id)
    }

    pub(super) fn generated_objects(&self, symbols: &[&str]) -> BTreeMap<String, String> {
        symbols
            .iter()
            .map(|symbol| ((*symbol).to_string(), self.symbol(symbol)))
            .collect()
    }
}
