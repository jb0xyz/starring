use std::collections::HashMap;
use std::fmt::{Debug, Formatter};
use std::sync::{Arc, Weak};

use authoring_application::{
    AuthoringAdmissionError, AuthoringTurnAdmissionPort, LocalAuthoringRequestKeyV1,
};
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};

const DEFAULT_MAX_KEYED_ENTRIES: usize = 4_096;
const DEFAULT_MODEL_CAPACITY: usize = 1;
const MAX_KEYED_ENTRIES: usize = 65_536;
const MAX_MODEL_CAPACITY: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AuthoringAdmissionConfigErrorV1 {
    #[error("authoring keyed admission capacity is invalid")]
    InvalidKeyedCapacity,
    #[error("authoring model admission capacity is invalid")]
    InvalidModelCapacity,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuthoringAdmissionConfigV1 {
    max_keyed_entries: usize,
    model_capacity: usize,
}

impl AuthoringAdmissionConfigV1 {
    pub fn new(
        max_keyed_entries: usize,
        model_capacity: usize,
    ) -> Result<Self, AuthoringAdmissionConfigErrorV1> {
        if !(1..=MAX_KEYED_ENTRIES).contains(&max_keyed_entries) {
            return Err(AuthoringAdmissionConfigErrorV1::InvalidKeyedCapacity);
        }
        if !(1..=MAX_MODEL_CAPACITY).contains(&model_capacity) {
            return Err(AuthoringAdmissionConfigErrorV1::InvalidModelCapacity);
        }
        Ok(Self {
            max_keyed_entries,
            model_capacity,
        })
    }

    pub fn model_capacity(self) -> usize {
        self.model_capacity
    }
}

impl Default for AuthoringAdmissionConfigV1 {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_KEYED_ENTRIES, DEFAULT_MODEL_CAPACITY)
            .expect("default authoring admission configuration is valid")
    }
}

#[derive(Clone)]
pub struct AuthoringAdmissionV1 {
    keyed: Arc<Mutex<HashMap<LocalAuthoringRequestKeyV1, Weak<Semaphore>>>>,
    model: Arc<Semaphore>,
    config: AuthoringAdmissionConfigV1,
}

impl AuthoringAdmissionV1 {
    pub fn new(config: AuthoringAdmissionConfigV1) -> Self {
        Self {
            keyed: Arc::new(Mutex::new(HashMap::new())),
            model: Arc::new(Semaphore::new(config.model_capacity)),
            config,
        }
    }

    pub fn production() -> Self {
        Self::new(AuthoringAdmissionConfigV1::default())
    }

    async fn keyed_semaphore(
        &self,
        key: &LocalAuthoringRequestKeyV1,
    ) -> Result<Arc<Semaphore>, AuthoringAdmissionError> {
        let mut keyed = self.keyed.lock().await;
        keyed.retain(|_, semaphore| semaphore.strong_count() != 0);
        if let Some(existing) = keyed.get(key).and_then(Weak::upgrade) {
            return Ok(existing);
        }
        if keyed.len() >= self.config.max_keyed_entries {
            return Err(AuthoringAdmissionError::Saturated);
        }
        let semaphore = Arc::new(Semaphore::new(1));
        keyed.insert(key.clone(), Arc::downgrade(&semaphore));
        Ok(semaphore)
    }
}

impl Default for AuthoringAdmissionV1 {
    fn default() -> Self {
        Self::production()
    }
}

impl Debug for AuthoringAdmissionV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AuthoringAdmissionV1(<redacted>)")
    }
}

impl AuthoringTurnAdmissionPort for AuthoringAdmissionV1 {
    type KeyedPermit = OwnedSemaphorePermit;
    type ModelPermit = OwnedSemaphorePermit;

    async fn acquire_keyed(
        &self,
        key: &LocalAuthoringRequestKeyV1,
    ) -> Result<Self::KeyedPermit, AuthoringAdmissionError> {
        self.keyed_semaphore(key)
            .await?
            .acquire_owned()
            .await
            .map_err(|_| AuthoringAdmissionError::Unavailable)
    }

    async fn acquire_model_capacity(&self) -> Result<Self::ModelPermit, AuthoringAdmissionError> {
        self.model
            .clone()
            .try_acquire_owned()
            .map_err(|_| AuthoringAdmissionError::Saturated)
    }
}

#[cfg(test)]
mod tests {
    use authoring_application::{
        AuthorizedInstallationScopeV1, ProductIdempotencyKeyV1, StartOrAdvanceAuthoringTurnV1,
    };
    use authoring_promotion::{
        AuthoringSessionId, AutomationInstallationId, PrincipalId, TenantId,
    };
    use discord_model::{GuildId, UserId};

    use super::*;

    fn key(idempotency_key: &str) -> LocalAuthoringRequestKeyV1 {
        let command = StartOrAdvanceAuthoringTurnV1::new(
            AuthoringSessionId::parse("session-1").unwrap(),
            authoring_application::AuthoringExpectedGenerationV1::new(0).unwrap(),
            ProductIdempotencyKeyV1::parse(idempotency_key).unwrap(),
            authoring_application::AuthoringHumanMessageV1::parse("Build a room").unwrap(),
        );
        LocalAuthoringRequestKeyV1::from_authorized_scope(
            PrincipalId::parse("principal-1").unwrap(),
            &AuthorizedInstallationScopeV1::from_fresh_authority(
                TenantId::parse("tenant-1").unwrap(),
                AutomationInstallationId::parse("installation-1").unwrap(),
                GuildId(1),
                UserId(2),
            ),
            &command,
        )
    }

    #[tokio::test]
    async fn same_key_waits_and_model_capacity_saturates_without_queueing() {
        let admission = AuthoringAdmissionV1::production();
        let first_keyed = admission.acquire_keyed(&key("request-1")).await.unwrap();
        let waiting_admission = admission.clone();
        let waiting = tokio::spawn(async move {
            waiting_admission
                .acquire_keyed(&key("request-1"))
                .await
                .unwrap()
        });
        tokio::task::yield_now().await;
        assert!(!waiting.is_finished());
        drop(first_keyed);
        let second_keyed = waiting.await.unwrap();
        drop(second_keyed);
        let model = admission.acquire_model_capacity().await.unwrap();
        assert_eq!(
            admission.acquire_model_capacity().await.unwrap_err(),
            AuthoringAdmissionError::Saturated
        );
        drop(model);
        assert!(admission.acquire_model_capacity().await.is_ok());
    }

    #[tokio::test]
    async fn dead_key_entries_are_pruned_before_the_bound_is_applied() {
        let admission = AuthoringAdmissionV1::new(AuthoringAdmissionConfigV1::new(1, 1).unwrap());
        drop(admission.acquire_keyed(&key("request-1")).await.unwrap());
        assert!(admission.acquire_keyed(&key("request-2")).await.is_ok());
    }
}
