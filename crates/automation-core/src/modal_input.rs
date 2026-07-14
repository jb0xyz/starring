use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use automation_state::{
    modal_input_utf16_len, InteractionRuleSet, ModalSpec, TriggerSpec,
    DISCORD_MODAL_INPUT_MAX_LENGTH,
};

use crate::event::{EventKind, RuntimeEvent};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModalInputError {
    ModalDefinitionMissing {
        modal: String,
    },
    RequiredMissing {
        modal: String,
        field: String,
    },
    Unexpected {
        modal: String,
        field: String,
    },
    TooShort {
        modal: String,
        field: String,
        min_length: u16,
        actual_length: usize,
    },
    TooLong {
        modal: String,
        field: String,
        max_length: u16,
        actual_length: usize,
    },
}

impl ModalInputError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::ModalDefinitionMissing { .. } => "MODAL_INPUT_DEFINITION_MISSING",
            Self::RequiredMissing { .. } => "MODAL_INPUT_MISSING",
            Self::Unexpected { .. } => "MODAL_INPUT_UNEXPECTED",
            Self::TooShort { .. } => "MODAL_INPUT_TOO_SHORT",
            Self::TooLong { .. } => "MODAL_INPUT_TOO_LONG",
        }
    }
}

impl fmt::Display for ModalInputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ModalDefinitionMissing { modal } => {
                write!(formatter, "{}: modal={modal}", self.code())
            }
            Self::RequiredMissing { modal, field } | Self::Unexpected { modal, field } => {
                write!(formatter, "{}: modal={modal}, field={field}", self.code())
            }
            Self::TooShort {
                modal,
                field,
                min_length,
                actual_length,
            } => write!(
                formatter,
                "{}: modal={modal}, field={field}, min_length={min_length}, actual_length={actual_length}",
                self.code()
            ),
            Self::TooLong {
                modal,
                field,
                max_length,
                actual_length,
            } => write!(
                formatter,
                "{}: modal={modal}, field={field}, max_length={max_length}, actual_length={actual_length}",
                self.code()
            ),
        }
    }
}

pub fn normalize_modal_submit_inputs(
    event: &RuntimeEvent,
    ruleset: &InteractionRuleSet,
) -> Result<Option<BTreeMap<String, String>>, ModalInputError> {
    let EventKind::ModalSubmit { modal, inputs } = &event.kind else {
        return Ok(None);
    };
    let has_trigger = ruleset.rules.iter().any(|rule| {
        matches!(
            &rule.trigger,
            TriggerSpec::ModalSubmit { modal: trigger_modal } if trigger_modal == modal
        )
    });
    if !has_trigger {
        return Ok(None);
    }
    let specification = ruleset
        .modals
        .iter()
        .find(|candidate| candidate.key == *modal)
        .ok_or_else(|| ModalInputError::ModalDefinitionMissing {
            modal: modal.clone(),
        })?;
    normalize_inputs(specification, inputs).map(Some)
}

fn normalize_inputs(
    modal: &ModalSpec,
    inputs: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, ModalInputError> {
    let expected: BTreeSet<&str> = modal
        .fields
        .iter()
        .map(|field| field.key.as_str())
        .collect();
    if let Some(field) = inputs
        .keys()
        .find(|field| !expected.contains(field.as_str()))
    {
        return Err(ModalInputError::Unexpected {
            modal: modal.key.clone(),
            field: field.clone(),
        });
    }

    let mut normalized = BTreeMap::new();
    for field in &modal.fields {
        let Some(value) = inputs.get(&field.key) else {
            if field.required {
                return Err(ModalInputError::RequiredMissing {
                    modal: modal.key.clone(),
                    field: field.key.clone(),
                });
            }
            normalized.insert(field.key.clone(), String::new());
            continue;
        };
        let value = field.input_policy.normalize(value);
        if value.is_empty() {
            if field.required {
                return Err(ModalInputError::RequiredMissing {
                    modal: modal.key.clone(),
                    field: field.key.clone(),
                });
            }
            normalized.insert(field.key.clone(), value);
            continue;
        }
        let actual_length = modal_input_utf16_len(&value);
        if let Some(min_length) = field.min_length {
            if actual_length < usize::from(min_length) {
                return Err(ModalInputError::TooShort {
                    modal: modal.key.clone(),
                    field: field.key.clone(),
                    min_length,
                    actual_length,
                });
            }
        }
        let max_length = field.max_length.unwrap_or(DISCORD_MODAL_INPUT_MAX_LENGTH);
        if actual_length > usize::from(max_length) {
            return Err(ModalInputError::TooLong {
                modal: modal.key.clone(),
                field: field.key.clone(),
                max_length,
                actual_length,
            });
        }
        normalized.insert(field.key.clone(), value);
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use automation_state::{ModalFieldSpec, ModalFieldStyle, ModalInputPolicy};

    use super::*;

    fn modal(field: ModalFieldSpec) -> ModalSpec {
        ModalSpec {
            key: "room".to_string(),
            title: "Room".to_string(),
            fields: vec![field],
        }
    }

    fn field() -> ModalFieldSpec {
        ModalFieldSpec {
            key: "name".to_string(),
            label: "Name".to_string(),
            style: ModalFieldStyle::Short,
            required: true,
            min_length: Some(2),
            max_length: Some(4),
            input_policy: ModalInputPolicy::Preserve,
        }
    }

    #[test]
    fn unicode_boundaries_use_utf16_code_units() {
        let specification = modal(field());
        assert_eq!(
            normalize_inputs(
                &specification,
                &BTreeMap::from([("name".to_string(), "😀".to_string())])
            )
            .unwrap()["name"],
            "😀"
        );
        let error = normalize_inputs(
            &specification,
            &BTreeMap::from([("name".to_string(), "😀😀😀".to_string())]),
        )
        .unwrap_err();
        assert_eq!(
            error,
            ModalInputError::TooLong {
                modal: "room".to_string(),
                field: "name".to_string(),
                max_length: 4,
                actual_length: 6,
            }
        );
    }

    #[test]
    fn trim_policy_is_explicit_and_precedes_length_validation() {
        let mut configured = field();
        configured.input_policy = ModalInputPolicy::TrimUnicodeWhitespace;
        let specification = modal(configured);
        let normalized = normalize_inputs(
            &specification,
            &BTreeMap::from([("name".to_string(), "  방a  ".to_string())]),
        )
        .unwrap();

        assert_eq!(normalized["name"], "방a");
    }

    #[test]
    fn preserve_policy_does_not_trim_identity_values() {
        let specification = modal(field());
        let normalized = normalize_inputs(
            &specification,
            &BTreeMap::from([("name".to_string(), " a ".to_string())]),
        )
        .unwrap();

        assert_eq!(normalized["name"], " a ");
    }

    #[test]
    fn optional_empty_and_absent_values_do_not_trigger_minimum_length() {
        let mut configured = field();
        configured.required = false;
        let specification = modal(configured);

        assert_eq!(
            normalize_inputs(&specification, &BTreeMap::new()).unwrap()["name"],
            ""
        );
        assert_eq!(
            normalize_inputs(
                &specification,
                &BTreeMap::from([("name".to_string(), String::new())])
            )
            .unwrap()["name"],
            ""
        );
    }
}
