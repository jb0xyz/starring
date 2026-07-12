use automation_instance_teardown::{
    DeleteOutcome, DeleterError, DeleterErrorKind, InstanceDeleter,
};
use discord_model::{ChannelId, GuildId, MessageId, RoleId};
use twilight_http::api_error::ApiError;
use twilight_http::error::ErrorType;
use twilight_http::Client;
use twilight_model::id::Id;

const UNKNOWN_CHANNEL: u64 = 10003;
const UNKNOWN_MESSAGE: u64 = 10008;
const UNKNOWN_ROLE: u64 = 10011;

pub struct TwilightInstanceDeleter<'a> {
    http: &'a Client,
}

impl<'a> TwilightInstanceDeleter<'a> {
    pub fn new(http: &'a Client) -> Self {
        Self { http }
    }
}

fn classify_delete_error_type(
    kind: &ErrorType,
    unknown_code: u64,
) -> Result<DeleteOutcome, DeleterErrorKind> {
    if let ErrorType::Response { error, status, .. } = kind {
        let code = match error {
            ApiError::General(error) => Some(error.code),
            _ => None,
        };
        return classify_delete_response(status.get(), code, unknown_code);
    }
    let kind = match kind {
        ErrorType::RequestCanceled | ErrorType::RequestError | ErrorType::RequestTimedOut => {
            DeleterErrorKind::Network
        }
        ErrorType::Unauthorized => DeleterErrorKind::Forbidden,
        _ => DeleterErrorKind::Unknown,
    };
    Err(kind)
}

fn classify_delete_response(
    status: u16,
    code: Option<u64>,
    unknown_code: u64,
) -> Result<DeleteOutcome, DeleterErrorKind> {
    if status == 404 && code == Some(unknown_code) {
        return Ok(DeleteOutcome::AlreadyGone);
    }
    Err(match status {
        429 => DeleterErrorKind::RateLimited,
        401 | 403 => DeleterErrorKind::Forbidden,
        _ => DeleterErrorKind::Unknown,
    })
}

fn classify_delete_error(
    error: &twilight_http::Error,
    unknown_code: u64,
) -> Result<DeleteOutcome, DeleterError> {
    classify_delete_error_type(error.kind(), unknown_code).map_err(|kind| DeleterError {
        kind,
        message: format!("twilight delete error: {error}"),
    })
}

impl InstanceDeleter for TwilightInstanceDeleter<'_> {
    async fn delete_message(
        &self,
        _: GuildId,
        channel: ChannelId,
        message: MessageId,
    ) -> Result<DeleteOutcome, DeleterError> {
        match self
            .http
            .delete_message(Id::new(channel.0), Id::new(message.0))
            .await
        {
            Ok(_) => Ok(DeleteOutcome::Deleted),
            Err(error) => classify_delete_error(&error, UNKNOWN_MESSAGE),
        }
    }

    async fn delete_channel(
        &self,
        _: GuildId,
        channel: ChannelId,
    ) -> Result<DeleteOutcome, DeleterError> {
        match self.http.delete_channel(Id::new(channel.0)).await {
            Ok(_) => Ok(DeleteOutcome::Deleted),
            Err(error) => classify_delete_error(&error, UNKNOWN_CHANNEL),
        }
    }

    async fn delete_role(
        &self,
        guild: GuildId,
        role: RoleId,
    ) -> Result<DeleteOutcome, DeleterError> {
        match self
            .http
            .delete_role(Id::new(guild.0), Id::new(role.0))
            .await
        {
            Ok(_) => Ok(DeleteOutcome::Deleted),
            Err(error) => classify_delete_error(&error, UNKNOWN_ROLE),
        }
    }
}

#[cfg(test)]
mod tests {
    use automation_instance_teardown::{DeleteOutcome, DeleterErrorKind};
    use twilight_http::error::ErrorType;

    use super::{classify_delete_error_type, classify_delete_response};

    #[test]
    fn exact_unknown_codes_are_already_gone() {
        for code in [10003, 10008, 10011] {
            assert_eq!(
                classify_delete_response(404, Some(code), code),
                Ok(DeleteOutcome::AlreadyGone)
            );
        }
    }

    #[test]
    fn wrong_unknown_code_is_not_already_gone() {
        assert_eq!(
            classify_delete_response(404, Some(10004), 10003),
            Err(DeleterErrorKind::Unknown)
        );
    }

    #[test]
    fn forbidden_rate_limit_and_network_are_classified() {
        assert_eq!(
            classify_delete_response(403, Some(50013), 10003),
            Err(DeleterErrorKind::Forbidden)
        );
        assert_eq!(
            classify_delete_response(429, None, 10003),
            Err(DeleterErrorKind::RateLimited)
        );
        assert_eq!(
            classify_delete_error_type(&ErrorType::RequestTimedOut, 10003),
            Err(DeleterErrorKind::Network)
        );
    }
}
