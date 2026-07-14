use schemars::{schema_for, JsonSchema};
use serde_json::{json, Map, Value};

pub(super) fn inline_schema_value<T: JsonSchema>() -> Value {
    let mut value = serde_json::to_value(schema_for!(T)).unwrap_or_else(|_| json!({}));
    let definitions = value
        .get("$defs")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    inline_references(&mut value, &definitions);
    if let Some(root) = value.as_object_mut() {
        root.remove("$schema");
        root.remove("$defs");
        root.remove("title");
        if let Some(expected_revision) = root
            .get_mut("properties")
            .and_then(Value::as_object_mut)
            .and_then(|properties| properties.get_mut("expected_revision"))
            .and_then(Value::as_object_mut)
        {
            expected_revision.remove("format");
            expected_revision.remove("minimum");
        }
    }
    value
}

fn inline_references(value: &mut Value, definitions: &Map<String, Value>) {
    let replacement = value
        .get("$ref")
        .and_then(Value::as_str)
        .and_then(|reference| reference.strip_prefix("#/$defs/"))
        .and_then(|name| definitions.get(name))
        .cloned();
    if let Some(mut replacement) = replacement {
        inline_references(&mut replacement, definitions);
        *value = replacement;
        return;
    }
    match value {
        Value::Array(values) => {
            for value in values {
                inline_references(value, definitions);
            }
        }
        Value::Object(values) => {
            for value in values.values_mut() {
                inline_references(value, definitions);
            }
        }
        _ => {}
    }
}
