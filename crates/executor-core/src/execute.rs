use crate::adapter::DiscordAdapter;

pub struct Executor<A: DiscordAdapter> {
    adapter: A,
}

impl<A: DiscordAdapter> Executor<A> {
    pub fn new(adapter: A) -> Self {
        Self { adapter }
    }

    pub fn adapter(&self) -> &A {
        &self.adapter
    }
}
