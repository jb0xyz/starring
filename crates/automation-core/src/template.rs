use std::collections::BTreeMap;

const EPHEMERAL_MAX_LEN: usize = 2000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SanitizeContext {
    EphemeralMessageContent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TemplateError {
    BadSyntax(String),
    UnsupportedVariable(String),
    MissingInput(String),
    TooLong { limit: usize, actual: usize },
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
        let sanitized = sanitize(&out, context);
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
    }
}

fn sanitize(input: &str, context: SanitizeContext) -> String {
    match context {
        SanitizeContext::EphemeralMessageContent => sanitize_message(input),
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
}
