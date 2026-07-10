use std::collections::BTreeMap;

const EPHEMERAL_MAX_LEN: usize = 2000;
const NAME_MAX_LEN: usize = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SanitizeContext {
    EphemeralMessageContent,
    ChannelName,
    RoleName,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TemplateError {
    BadSyntax(String),
    UnsupportedVariable(String),
    MissingInput(String),
    TooLong { limit: usize, actual: usize },
    EmptyAfterSanitize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Segment {
    Literal(String),
    Input(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TemplateString {
    segments: Vec<Segment>,
}

impl TemplateString {
    pub fn parse(source: &str) -> Result<TemplateString, TemplateError> {
        let mut segments = Vec::new();
        let mut rest = source;
        while let Some(start) = rest.find("${") {
            let literal = &rest[..start];
            if !literal.is_empty() {
                segments.push(Segment::Literal(literal.to_string()));
            }
            let after = &rest[start + 2..];
            let end = after
                .find('}')
                .ok_or_else(|| TemplateError::BadSyntax(source.to_string()))?;
            let expr = &after[..end];
            let key = expr
                .strip_prefix("input.")
                .ok_or_else(|| TemplateError::UnsupportedVariable(expr.to_string()))?;
            if key.is_empty() {
                return Err(TemplateError::BadSyntax(source.to_string()));
            }
            segments.push(Segment::Input(key.to_string()));
            rest = &after[end + 1..];
        }
        if !rest.is_empty() {
            segments.push(Segment::Literal(rest.to_string()));
        }
        Ok(TemplateString { segments })
    }

    pub fn input_keys(&self) -> Vec<&str> {
        self.segments
            .iter()
            .filter_map(|segment| match segment {
                Segment::Input(key) => Some(key.as_str()),
                Segment::Literal(_) => None,
            })
            .collect()
    }

    pub fn render(
        &self,
        inputs: &BTreeMap<String, String>,
        context: SanitizeContext,
    ) -> Result<String, TemplateError> {
        let mut out = String::new();
        for segment in &self.segments {
            match segment {
                Segment::Literal(text) => out.push_str(text),
                Segment::Input(key) => {
                    let value = inputs
                        .get(key)
                        .ok_or_else(|| TemplateError::MissingInput(key.clone()))?;
                    out.push_str(value);
                }
            }
        }
        let sanitized = sanitize(&out, context)?;
        let limit = max_len(context);
        let actual = sanitized.chars().count();
        if actual > limit {
            return Err(TemplateError::TooLong { limit, actual });
        }
        Ok(sanitized)
    }
}

fn max_len(context: SanitizeContext) -> usize {
    match context {
        SanitizeContext::EphemeralMessageContent => EPHEMERAL_MAX_LEN,
        SanitizeContext::ChannelName | SanitizeContext::RoleName => NAME_MAX_LEN,
    }
}

fn sanitize(input: &str, context: SanitizeContext) -> Result<String, TemplateError> {
    let result = match context {
        SanitizeContext::EphemeralMessageContent => sanitize_message(input),
        SanitizeContext::ChannelName => sanitize_channel_name(input),
        SanitizeContext::RoleName => sanitize_role_name(input),
    };
    if result.is_empty() {
        Err(TemplateError::EmptyAfterSanitize)
    } else {
        Ok(result)
    }
}

fn sanitize_message(input: &str) -> String {
    let replaced = input
        .replace("@everyone", "@\u{200b}everyone")
        .replace("@here", "@\u{200b}here")
        .replace("<@", "<\u{200b}@")
        .replace("<#", "<\u{200b}#");
    replaced
        .chars()
        .filter(|character| *character == '\n' || !character.is_control())
        .collect()
}

fn sanitize_channel_name(input: &str) -> String {
    let mut result = String::new();
    for character in input.chars().flat_map(char::to_lowercase) {
        if character.is_ascii_lowercase() || character.is_ascii_digit() {
            result.push(character);
        } else if !result.ends_with('-') {
            result.push('-');
        }
    }
    result.trim_matches('-').to_string()
}

fn sanitize_role_name(input: &str) -> String {
    let neutralized = input
        .replace("@everyone", "@\u{200b}everyone")
        .replace("@here", "@\u{200b}here")
        .replace("<@", "<\u{200b}@")
        .replace("<#", "<\u{200b}#");
    let cleaned: String = neutralized
        .chars()
        .filter(|character| !character.is_control())
        .collect();
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect()
    }

    fn render(source: &str, pairs: &[(&str, &str)]) -> Result<String, TemplateError> {
        TemplateString::parse(source)?
            .render(&inputs(pairs), SanitizeContext::EphemeralMessageContent)
    }

    #[test]
    fn parse_literal_only() {
        let template = TemplateString::parse("hello world").unwrap();
        assert!(template.input_keys().is_empty());
    }

    #[test]
    fn parse_rejects_unclosed() {
        assert_eq!(
            TemplateString::parse("hi ${input.x").unwrap_err(),
            TemplateError::BadSyntax("hi ${input.x".to_string())
        );
    }

    #[test]
    fn parse_rejects_wrong_prefix() {
        assert_eq!(
            TemplateString::parse("${actor.id}").unwrap_err(),
            TemplateError::UnsupportedVariable("actor.id".to_string())
        );
    }

    #[test]
    fn parse_rejects_empty_key() {
        assert!(matches!(
            TemplateString::parse("${input.}").unwrap_err(),
            TemplateError::BadSyntax(_)
        ));
    }

    #[test]
    fn input_keys_extracted() {
        let template = TemplateString::parse("${input.a}-${input.b}").unwrap();
        assert_eq!(template.input_keys(), vec!["a", "b"]);
    }

    #[test]
    fn render_literal_unchanged() {
        assert_eq!(render("welcome", &[]).unwrap(), "welcome");
    }

    #[test]
    fn render_substitutes_inputs() {
        assert_eq!(
            render(
                "room: ${input.name} / ${input.owner}",
                &[("name", "cozy"), ("owner", "kim")]
            )
            .unwrap(),
            "room: cozy / kim"
        );
    }

    #[test]
    fn render_missing_input_errors() {
        assert_eq!(
            render("${input.x}", &[]).unwrap_err(),
            TemplateError::MissingInput("x".to_string())
        );
    }

    #[test]
    fn render_neutralizes_everyone_and_here() {
        let out = render("${input.x}", &[("x", "@everyone @here")]).unwrap();
        assert!(!out.contains("@everyone"));
        assert!(!out.contains("@here"));
    }

    #[test]
    fn render_neutralizes_mentions() {
        let out = render("${input.x}", &[("x", "<@123> <@&456> <#789>")]).unwrap();
        assert!(!out.contains("<@"));
        assert!(!out.contains("<#"));
    }

    #[test]
    fn render_preserves_markdown() {
        let out = render("${input.x}", &[("x", "**bold** _em_")]).unwrap();
        assert!(out.contains("**bold**"));
        assert!(out.contains("_em_"));
    }

    #[test]
    fn render_too_long_errors() {
        let long = "a".repeat(2001);
        assert!(matches!(
            render("${input.x}", &[("x", long.as_str())]).unwrap_err(),
            TemplateError::TooLong { .. }
        ));
    }

    #[test]
    fn parse_rejects_created_variable() {
        assert_eq!(
            TemplateString::parse("${created.channel.id}").unwrap_err(),
            TemplateError::UnsupportedVariable("created.channel.id".to_string())
        );
    }

    fn channel(input: &str) -> Result<String, TemplateError> {
        TemplateString::parse("${input.x}")?
            .render(&inputs(&[("x", input)]), SanitizeContext::ChannelName)
    }

    fn role(input: &str) -> Result<String, TemplateError> {
        TemplateString::parse("${input.x}")?
            .render(&inputs(&[("x", input)]), SanitizeContext::RoleName)
    }

    #[test]
    fn channel_name_spaces_to_hyphens() {
        assert_eq!(channel("study room").unwrap(), "study-room");
    }

    #[test]
    fn channel_name_lowercased() {
        assert_eq!(channel("Study Room 1").unwrap(), "study-room-1");
    }

    #[test]
    fn channel_name_removes_invalid_chars() {
        assert_eq!(channel("study!@#room").unwrap(), "study-room");
    }

    #[test]
    fn channel_name_empty_after_sanitize_errors() {
        assert_eq!(
            channel("수학").unwrap_err(),
            TemplateError::EmptyAfterSanitize
        );
        assert_eq!(
            channel("!!!!").unwrap_err(),
            TemplateError::EmptyAfterSanitize
        );
    }

    #[test]
    fn channel_name_too_long_errors() {
        assert!(matches!(
            channel(&"a".repeat(101)).unwrap_err(),
            TemplateError::TooLong { .. }
        ));
    }

    #[test]
    fn role_name_keeps_hangul() {
        assert_eq!(role("수학 스터디 멤버").unwrap(), "수학 스터디 멤버");
    }

    #[test]
    fn role_name_neutralizes_everyone() {
        let out = role("@everyone 멤버").unwrap();
        assert!(!out.contains("@everyone"));
        assert!(out.contains("멤버"));
    }

    #[test]
    fn role_name_too_long_errors() {
        assert!(matches!(
            role(&"가".repeat(101)).unwrap_err(),
            TemplateError::TooLong { .. }
        ));
    }
}
