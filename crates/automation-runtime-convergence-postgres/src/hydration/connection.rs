use sqlx::pool::PoolConnection;
use sqlx::{PgConnection, Postgres};

pub(super) struct ExactTargetConnectionGuardV1 {
    connection: Option<PoolConnection<Postgres>>,
}

impl ExactTargetConnectionGuardV1 {
    pub(super) fn new(connection: PoolConnection<Postgres>) -> Self {
        Self {
            connection: Some(connection),
        }
    }

    pub(super) fn connection_mut(&mut self) -> Option<&mut PgConnection> {
        self.connection.as_deref_mut()
    }

    pub(super) fn release_to_pool(mut self) {
        if let Some(connection) = self.connection.take() {
            drop(connection);
        }
    }
}

impl Drop for ExactTargetConnectionGuardV1 {
    fn drop(&mut self) {
        if let Some(connection) = self.connection.take() {
            drop(connection.detach());
        }
    }
}
