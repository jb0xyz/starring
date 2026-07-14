use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModalSpec {
    pub key: String,
    pub title: String,
    #[serde(default)]
    pub fields: Vec<ModalFieldSpec>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModalFieldSpec {
    pub key: String,
    pub label: String,
    pub style: ModalFieldStyle,
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_length: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_length: Option<u16>,
    #[serde(default, skip_serializing_if = "ModalInputPolicy::is_preserve")]
    pub input_policy: ModalInputPolicy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModalFieldStyle {
    Short,
    Paragraph,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModalInputPolicy {
    #[default]
    Preserve,
    TrimUnicodeWhitespace,
}

impl ModalInputPolicy {
    fn is_preserve(&self) -> bool {
        matches!(self, Self::Preserve)
    }

    pub fn normalize(self, value: &str) -> String {
        match self {
            Self::Preserve => value.to_string(),
            Self::TrimUnicodeWhitespace => value.trim().to_string(),
        }
    }
}

pub const DISCORD_MODAL_INPUT_MAX_LENGTH: u16 = 4_000;

pub fn modal_input_utf16_len(value: &str) -> usize {
    value.encode_utf16().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ModalSpec {
        ModalSpec {
            key: "study_room_modal".to_string(),
            title: "Create study room".to_string(),
            fields: vec![ModalFieldSpec {
                key: "room_name".to_string(),
                label: "Room name".to_string(),
                style: ModalFieldStyle::Short,
                required: true,
                min_length: None,
                max_length: None,
                input_policy: ModalInputPolicy::Preserve,
            }],
        }
    }

    #[test]
    fn modal_spec_roundtrips() {
        let modal = sample();
        let json = serde_json::to_string(&modal).unwrap();
        let back: ModalSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(modal, back);
        assert!(json.contains(r#""style":"short""#));
        assert!(!json.contains("min_length"));
        assert!(!json.contains("max_length"));
        assert!(!json.contains("input_policy"));
    }

    #[test]
    fn legacy_modal_field_wire_shape_is_unchanged() {
        let json = r#"{"key":"room_name","label":"Room name","style":"short","required":true}"#;
        let field: ModalFieldSpec = serde_json::from_str(json).unwrap();

        assert_eq!(field.min_length, None);
        assert_eq!(field.max_length, None);
        assert_eq!(field.input_policy, ModalInputPolicy::Preserve);
        assert_eq!(serde_json::to_string(&field).unwrap(), json);
    }

    #[test]
    fn bounded_contract_roundtrips() {
        let json = r#"{"key":"room_name","label":"Room name","style":"short","required":true,"min_length":2,"max_length":40,"input_policy":"trim_unicode_whitespace"}"#;
        let field: ModalFieldSpec = serde_json::from_str(json).unwrap();

        assert_eq!(field.min_length, Some(2));
        assert_eq!(field.max_length, Some(40));
        assert_eq!(field.input_policy, ModalInputPolicy::TrimUnicodeWhitespace);
        assert_eq!(serde_json::to_string(&field).unwrap(), json);
    }

    #[test]
    fn modal_input_length_uses_utf16_code_units() {
        assert_eq!(modal_input_utf16_len("abc"), 3);
        assert_eq!(modal_input_utf16_len("한글"), 2);
        assert_eq!(modal_input_utf16_len("😀"), 2);
        assert_eq!(modal_input_utf16_len("👨‍👩‍👧‍👦"), 11);
    }

    #[test]
    fn unknown_field_in_modal_is_rejected() {
        let json = r#"{"key":"m","title":"t","fields":[],"evil":1}"#;
        assert!(serde_json::from_str::<ModalSpec>(json).is_err());
    }
}
