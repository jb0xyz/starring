use automation_ruleset_activation::{ActivationRequestId, ApplyAttemptId};

use crate::random_instance_id::encode_instance_id;

fn random_value(prefix: &str) -> Result<String, String> {
    let mut bytes = [0u8; 8];
    getrandom::getrandom(&mut bytes)
        .map_err(|_| "activation id entropy unavailable".to_string())?;
    Ok(format!("{prefix}_{}", encode_instance_id(bytes)))
}

pub fn random_request_id() -> Result<ActivationRequestId, String> {
    let value = random_value("ar")?;
    ActivationRequestId::parse(&value)
        .map_err(|error| format!("generated activation request id is invalid: {error}"))
}

pub fn random_attempt_id() -> Result<ApplyAttemptId, String> {
    let value = random_value("at")?;
    ApplyAttemptId::parse(&value)
        .map_err(|error| format!("generated activation attempt id is invalid: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_request_id_has_valid_shape() {
        let id = random_request_id().unwrap();
        assert!(id.as_str().starts_with("ar_"));
        assert_eq!(id.as_str().len(), 15);
        assert_eq!(ActivationRequestId::parse(id.as_str()).unwrap(), id);
    }

    #[test]
    fn generated_attempt_id_has_valid_shape() {
        let id = random_attempt_id().unwrap();
        assert!(id.as_str().starts_with("at_"));
        assert_eq!(id.as_str().len(), 15);
        assert_eq!(ApplyAttemptId::parse(id.as_str()).unwrap(), id);
    }

    #[test]
    fn generated_ids_vary() {
        assert_ne!(random_request_id().unwrap(), random_request_id().unwrap());
        assert_ne!(random_attempt_id().unwrap(), random_attempt_id().unwrap());
    }
}
