use crate::AuthorityOperatorErrorV1;

const BINDING_KEY: &str = "community_hub";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthorityAdvanceCommandValuesV1 {
    pub system_identifier: String,
    pub installation_id: String,
    pub channel_id: String,
    pub acknowledgement: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthorityAdvanceCommandV1 {
    system_identifier: String,
    installation_id: String,
    channel_id: u64,
}

impl AuthorityAdvanceCommandV1 {
    pub fn parse(
        values: AuthorityAdvanceCommandValuesV1,
    ) -> Result<Self, AuthorityOperatorErrorV1> {
        if values.system_identifier.is_empty()
            || values.system_identifier.len() > 20
            || values.system_identifier.starts_with('0')
            || !values
                .system_identifier
                .bytes()
                .all(|byte| byte.is_ascii_digit())
            || !bounded_identifier(&values.installation_id)
        {
            return Err(AuthorityOperatorErrorV1::Acknowledgement);
        }
        let channel_id = values
            .channel_id
            .parse::<u64>()
            .ok()
            .filter(|channel_id| *channel_id != 0)
            .filter(|channel_id| channel_id.to_string() == values.channel_id)
            .ok_or(AuthorityOperatorErrorV1::Acknowledgement)?;
        let expected = format!(
            "starring-staging-authority-advance-v1:{}:{}:1:2:{}:{}:reviewed-discord-text-channel",
            values.system_identifier, values.installation_id, BINDING_KEY, values.channel_id
        );
        if values.acknowledgement != expected {
            return Err(AuthorityOperatorErrorV1::Acknowledgement);
        }
        Ok(Self {
            system_identifier: values.system_identifier,
            installation_id: values.installation_id,
            channel_id,
        })
    }

    pub fn system_identifier(&self) -> &str {
        &self.system_identifier
    }

    pub fn installation_id(&self) -> &str {
        &self.installation_id
    }

    pub const fn channel_id(&self) -> u64 {
        self.channel_id
    }
}

pub(crate) const fn binding_key() -> &'static str {
    BINDING_KEY
}

fn bounded_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b':' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values() -> AuthorityAdvanceCommandValuesV1 {
        AuthorityAdvanceCommandValuesV1 {
            system_identifier: "7663763942264209752".to_string(),
            installation_id: "installation.staging".to_string(),
            channel_id: "123456789012345678".to_string(),
            acknowledgement: "starring-staging-authority-advance-v1:7663763942264209752:installation.staging:1:2:community_hub:123456789012345678:reviewed-discord-text-channel".to_string(),
        }
    }

    #[test]
    fn exact_acknowledgement_binds_every_operator_choice() {
        let command = AuthorityAdvanceCommandV1::parse(values()).unwrap();
        assert_eq!(command.system_identifier(), "7663763942264209752");
        assert_eq!(command.installation_id(), "installation.staging");
        assert_eq!(command.channel_id(), 123456789012345678);

        for mutate in [
            |values: &mut AuthorityAdvanceCommandValuesV1| {
                values.system_identifier = "7663763942264209753".to_string()
            },
            |values: &mut AuthorityAdvanceCommandValuesV1| {
                values.installation_id = "installation.other".to_string()
            },
            |values: &mut AuthorityAdvanceCommandValuesV1| {
                values.channel_id = "123456789012345679".to_string()
            },
        ] {
            let mut changed = values();
            mutate(&mut changed);
            assert_eq!(
                AuthorityAdvanceCommandV1::parse(changed),
                Err(AuthorityOperatorErrorV1::Acknowledgement)
            );
        }
    }

    #[test]
    fn noncanonical_identifiers_and_snowflakes_fail_closed() {
        for installation_id in ["", "invalid installation", &"a".repeat(129)] {
            let mut changed = values();
            changed.installation_id = installation_id.to_string();
            assert_eq!(
                AuthorityAdvanceCommandV1::parse(changed),
                Err(AuthorityOperatorErrorV1::Acknowledgement)
            );
        }
        for channel_id in ["0", "01", "-1", "18446744073709551616"] {
            let mut changed = values();
            changed.channel_id = channel_id.to_string();
            assert_eq!(
                AuthorityAdvanceCommandV1::parse(changed),
                Err(AuthorityOperatorErrorV1::Acknowledgement)
            );
        }
    }
}
