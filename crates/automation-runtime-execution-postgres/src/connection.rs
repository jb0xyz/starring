use sqlx::pool::PoolConnection;
use sqlx::{PgConnection, Postgres};

pub(crate) struct ExecutionConnectionGuardV1 {
    connection: Option<PoolConnection<Postgres>>,
}

impl ExecutionConnectionGuardV1 {
    pub(crate) fn new(connection: PoolConnection<Postgres>) -> Self {
        Self {
            connection: Some(connection),
        }
    }

    pub(crate) fn connection_mut(&mut self) -> Option<&mut PgConnection> {
        self.connection.as_deref_mut()
    }

    pub(crate) fn release_to_pool(mut self) {
        if let Some(connection) = self.connection.take() {
            drop(connection);
        }
    }
}

impl Drop for ExecutionConnectionGuardV1 {
    fn drop(&mut self) {
        if let Some(connection) = self.connection.take() {
            drop(connection.detach());
        }
    }
}
