use crate::llm::{Message, MessageRole};

use super::super::LimitKind;
use super::state::INTENT_HUMAN_PREFIX;

pub(super) const MAX_INTENT_RESTORED_TRANSCRIPT_CHARS: usize = 4 * 1024 * 1024;
const MAX_INTENT_RESTORED_TRANSCRIPT_TURNS: usize = 1024;
pub(super) const MAX_INTENT_RESTORED_FAILURE_RESULTS: usize = 16;
const MAX_INTENT_RESTORED_HUMAN_BYTES: usize = 512 * 1024;

pub(super) fn durable_transcript_violation(messages: &[Message]) -> Option<LimitKind> {
    durable_transcript_violation_iter(messages.iter())
}

pub(super) fn durable_transcript_violation_with_added(
    messages: &[Message],
    message: &Message,
) -> Option<LimitKind> {
    durable_transcript_violation_iter(messages.iter().chain(std::iter::once(message)))
}

fn durable_transcript_violation_iter<'a>(
    messages: impl Iterator<Item = &'a Message>,
) -> Option<LimitKind> {
    let mut chars = 0usize;
    let mut turns = 0usize;
    let mut failures = 0usize;
    let mut human_bytes = 0usize;
    for message in messages {
        let Some(next_chars) = chars
            .checked_add(message.estimated_chars())
            .and_then(|value| value.checked_add(96))
        else {
            return Some(LimitKind::DurableTranscriptChars);
        };
        chars = next_chars;
        if chars > MAX_INTENT_RESTORED_TRANSCRIPT_CHARS {
            return Some(LimitKind::DurableTranscriptChars);
        }
        if message.role == MessageRole::User
            && message.tool_call_id.is_none()
            && message.tool_calls.is_empty()
            && message.content.starts_with(INTENT_HUMAN_PREFIX)
        {
            let Some(next_human_bytes) = human_bytes.checked_add(message.content.len()) else {
                return Some(LimitKind::DurableTranscriptReplayWork);
            };
            human_bytes = next_human_bytes;
            if human_bytes > MAX_INTENT_RESTORED_HUMAN_BYTES {
                return Some(LimitKind::DurableTranscriptReplayWork);
            }
            let Some(next_turns) = turns.checked_add(1) else {
                return Some(LimitKind::DurableTranscriptReplayWork);
            };
            turns = next_turns;
            if turns > MAX_INTENT_RESTORED_TRANSCRIPT_TURNS {
                return Some(LimitKind::DurableTranscriptReplayWork);
            }
        }
        if is_failure_result(message) {
            let Some(next_failures) = failures.checked_add(1) else {
                return Some(LimitKind::DurableTranscriptReplayWork);
            };
            failures = next_failures;
            if failures > MAX_INTENT_RESTORED_FAILURE_RESULTS {
                return Some(LimitKind::DurableTranscriptReplayWork);
            }
        }
    }
    None
}

fn is_failure_result(message: &Message) -> bool {
    message.role == MessageRole::Tool
        && serde_json::from_str::<serde_json::Value>(&message.content)
            .ok()
            .and_then(|value| value.get("ok").and_then(serde_json::Value::as_bool))
            == Some(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_work_bounds_turns_and_failure_results() {
        let exact_failures = (0..MAX_INTENT_RESTORED_FAILURE_RESULTS)
            .map(|index| Message::tool(index.to_string(), r#"{"ok":false}"#))
            .collect::<Vec<_>>();
        assert_eq!(durable_transcript_violation(&exact_failures), None);

        let mut excessive_failures = exact_failures;
        excessive_failures.push(Message::tool("overflow", r#"{"ok":false}"#));
        assert_eq!(
            durable_transcript_violation(&excessive_failures),
            Some(LimitKind::DurableTranscriptReplayWork)
        );

        let excessive_turns = (0..=MAX_INTENT_RESTORED_TRANSCRIPT_TURNS)
            .map(|index| {
                Message::user(format!(
                    "{INTENT_HUMAN_PREFIX}{{\"text\":\"turn-{index}\"}}"
                ))
            })
            .collect::<Vec<_>>();
        assert_eq!(
            durable_transcript_violation(&excessive_turns),
            Some(LimitKind::DurableTranscriptReplayWork)
        );
    }

    #[test]
    fn replay_work_bounds_cumulative_human_grounding_bytes() {
        let content_len = MAX_INTENT_RESTORED_HUMAN_BYTES / 8;
        let exact = (0..8)
            .map(|_| intent_human_with_content_len(content_len))
            .collect::<Vec<_>>();
        assert_eq!(
            exact
                .iter()
                .map(|message| message.content.len())
                .sum::<usize>(),
            MAX_INTENT_RESTORED_HUMAN_BYTES
        );
        assert_eq!(durable_transcript_violation(&exact), None);

        let mut excessive = exact;
        excessive.push(intent_human_with_content_len(64));
        assert_eq!(
            durable_transcript_violation(&excessive),
            Some(LimitKind::DurableTranscriptReplayWork)
        );
    }

    fn intent_human_with_content_len(content_len: usize) -> Message {
        let empty = format!(r#"{INTENT_HUMAN_PREFIX}{{"text":""}}"#);
        let filler_len = content_len.checked_sub(empty.len()).unwrap();
        let message = Message::user(format!(
            r#"{INTENT_HUMAN_PREFIX}{{"text":"{}"}}"#,
            "x".repeat(filler_len)
        ));
        assert_eq!(message.content.len(), content_len);
        message
    }
}
