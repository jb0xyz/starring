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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModalFieldStyle {
    Short,
    Paragraph,
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
    }

    #[test]
    fn unknown_field_in_modal_is_rejected() {
        let json = r#"{"key":"m","title":"t","fields":[],"evil":1}"#;
        assert!(serde_json::from_str::<ModalSpec>(json).is_err());
    }
}
