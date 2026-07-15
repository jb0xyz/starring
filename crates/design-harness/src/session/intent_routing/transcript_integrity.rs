use crate::intent::identity::LengthFramedDigest;
use crate::llm::{Message, MessageRole};

const TRANSCRIPT_INTEGRITY_DIGEST_DOMAIN_V1: &[u8] = b"starring.intent.transcript_integrity.v1\0";

pub(super) fn intent_transcript_integrity_digest(messages: &[Message]) -> String {
    let message_count =
        u64::try_from(messages.len()).expect("intent transcript length exceeds u64");
    let mut digest = LengthFramedDigest::new(TRANSCRIPT_INTEGRITY_DIGEST_DOMAIN_V1);
    digest.update(&message_count.to_be_bytes());
    for (index, message) in messages.iter().enumerate() {
        let index = u64::try_from(index).expect("intent transcript index exceeds u64");
        digest.update(b"message");
        digest.update(&index.to_be_bytes());
        digest.update(message_role_wire(message.role));
        digest.update(message.content.as_bytes());
        digest.update(option_presence(message.tool_call_id.as_ref()));
        digest.update(
            message
                .tool_call_id
                .as_deref()
                .unwrap_or_default()
                .as_bytes(),
        );
        let call_count =
            u64::try_from(message.tool_calls.len()).expect("intent tool call count exceeds u64");
        digest.update(&call_count.to_be_bytes());
        for call in &message.tool_calls {
            digest.update(b"tool_call");
            digest.update(call.id.as_bytes());
            digest.update(call.name.as_bytes());
            digest.update(call.arguments.as_bytes());
        }
    }
    digest.finalize()
}

fn message_role_wire(role: MessageRole) -> &'static [u8] {
    match role {
        MessageRole::System => b"system",
        MessageRole::User => b"user",
        MessageRole::Assistant => b"assistant",
        MessageRole::Tool => b"tool",
    }
}

fn option_presence<T>(value: Option<&T>) -> &'static [u8] {
    if value.is_some() {
        b"some"
    } else {
        b"none"
    }
}

#[cfg(test)]
mod tests {
    use crate::llm::{Message, MessageRole, ToolCall};

    use super::intent_transcript_integrity_digest;

    fn transcript() -> Vec<Message> {
        vec![
            Message::system("fixed"),
            Message::user("request"),
            Message::assistant_tool_calls(vec![ToolCall {
                id: "call-1".to_string(),
                name: "interpret_intent_core".to_string(),
                arguments: "{\"expected_revision\":0}".to_string(),
            }]),
            Message::tool("call-1", "{\"ok\":false}"),
        ]
    }

    #[test]
    fn transcript_digests_are_golden_and_stable() {
        assert_eq!(
            intent_transcript_integrity_digest(&[]),
            "09f9dae41c4750ba5d60423b722f386bafb505424e28b3a78add421030b0abd3"
        );
        assert_eq!(
            intent_transcript_integrity_digest(&transcript()),
            "f81db444c4aba3b11783c769fcb51b3e48795e7fe5baa3a460b5bc1055a1d6f7"
        );
    }

    #[test]
    fn every_message_and_tool_call_surface_rotates_integrity() {
        let base = transcript();
        let digest = intent_transcript_integrity_digest(&base);
        let mut variants = Vec::new();

        let mut changed = base.clone();
        changed[1].role = MessageRole::Assistant;
        variants.push(changed);

        let mut changed = base.clone();
        changed[1].content.push('!');
        variants.push(changed);

        let mut changed = base.clone();
        changed[3].tool_call_id = Some("call-2".to_string());
        variants.push(changed);

        let mut changed = base.clone();
        changed[3].tool_call_id = None;
        variants.push(changed);

        let mut changed = base.clone();
        changed[2].tool_calls[0].id.push('x');
        variants.push(changed);

        let mut changed = base.clone();
        changed[2].tool_calls[0].name.push('x');
        variants.push(changed);

        let mut changed = base.clone();
        changed[2].tool_calls[0].arguments.push(' ');
        variants.push(changed);

        let mut changed = base.clone();
        changed.swap(1, 2);
        variants.push(changed);

        let mut changed = base.clone();
        changed.push(Message::assistant("extra"));
        variants.push(changed);

        for variant in variants {
            assert_ne!(intent_transcript_integrity_digest(&variant), digest);
        }
    }

    #[test]
    fn exact_result_bytes_are_not_json_canonicalized() {
        let base = transcript();
        let mut whitespace = base.clone();
        whitespace[3].content = "{ \"ok\": false }".to_string();
        let mut reordered = base.clone();
        reordered[3].content = "{\"code\":\"X\",\"ok\":false}".to_string();

        assert_ne!(
            intent_transcript_integrity_digest(&whitespace),
            intent_transcript_integrity_digest(&base)
        );
        assert_ne!(
            intent_transcript_integrity_digest(&reordered),
            intent_transcript_integrity_digest(&base)
        );
    }
}
