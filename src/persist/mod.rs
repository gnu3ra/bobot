use async_trait::async_trait;

pub(crate) type Result<T> = anyhow::Result<T>;

pub(crate) mod redis;

pub(crate) mod entity;

#[async_trait]
pub trait DbTable<T> {
    async fn insert(&self, pool: &T, wrapper: Option<String>) -> Result<()>;
}
