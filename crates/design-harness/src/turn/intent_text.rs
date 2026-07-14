use crate::errors::StructuredError;

pub(super) fn normalized_required_text(
    value: &str,
    max_chars: usize,
    multiline: bool,
    reject_template: bool,
    path: &str,
) -> Result<String, StructuredError> {
    let normalized = value.trim().to_string();
    if normalized.is_empty() {
        return Err(intent_error(
            "EMPTY_INTENT_TEXT",
            path,
            "An intent interpretation text value is empty",
            "Provide a non-empty semantic value",
        ));
    }
    validate_text_shape(&normalized, max_chars, multiline, reject_template, path)?;
    Ok(normalized)
}

pub(super) fn validate_text_shape(
    value: &str,
    max_chars: usize,
    multiline: bool,
    reject_template: bool,
    path: &str,
) -> Result<(), StructuredError> {
    if value.encode_utf16().count() > max_chars {
        return Err(intent_error(
            "INTENT_TEXT_TOO_LONG",
            path,
            format!("The intent text exceeds {max_chars} characters"),
            "Shorten the value",
        ));
    }
    if value.chars().any(|character| {
        (character.is_control() && !(multiline && character == '\n'))
            || is_directional_control(character)
    }) {
        return Err(intent_error(
            "INVALID_INTENT_TEXT_CONTROL",
            path,
            "The intent text contains an unsupported control character",
            "Remove line breaks, directional controls, or null characters from this value",
        ));
    }
    if reject_template && value.contains("${") {
        return Err(intent_error(
            "RAW_INTENT_TEMPLATE_FORBIDDEN",
            path,
            "Raw template syntax is not allowed in Intent IR",
            "Use the typed room-name prefix and suffix fields",
        ));
    }
    Ok(())
}

fn is_directional_control(character: char) -> bool {
    matches!(
        character,
        '\u{061c}'
            | '\u{200e}'
            | '\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2066}'..='\u{2069}'
    )
}

fn intent_error(
    code: impl Into<String>,
    location: impl Into<String>,
    message: impl Into<String>,
    hint: impl Into<String>,
) -> StructuredError {
    StructuredError {
        code: code.into(),
        location: location.into(),
        message: message.into(),
        hint: hint.into(),
    }
}
