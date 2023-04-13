use std::{future::Future, sync::Arc};

use anyhow::Result;
use redis::{AsyncCommands, Client, FromRedisValue};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tokio::task::JoinHandle;
use uuid::Uuid;

const JOBS_KEY: &str = "jobs";
const STATUS_EXPIRE_SECONDS: usize = 60 * 60 * 24; // 24 hours

// TODO: Implement a proper job queue/explore limitations of this particular design.
//       Also, redis or rabbitmq? Redis is not used for anything else, so rabbitmq
//       might be better.

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Job<D> {
    pub id: Uuid,
    pub data: D,
}

pub struct JobQueue<D, C> {
    client: Client,
    _phantom: std::marker::PhantomData<(D, C)>,
}

impl<D, C> JobQueue<D, C>
where
    D: Serialize + DeserializeOwned + Send + 'static,
    C: Send + Sync + 'static,
{
    pub fn new(url: &str) -> Result<Self> {
        let client = Client::open(url)?;
        Ok(Self {
            client,
            _phantom: Default::default(),
        })
    }

    pub fn start<F, Fut>(&self, ctx: Arc<C>, f: F) -> Result<JoinHandle<Result<()>>>
    where
        Fut: Future<Output = Result<()>> + Send + 'static,
        F: Fn(Job<D>, Arc<C>) -> Fut + Send + Sync + 'static,
    {
        let client = self.client.clone();
        let handle = tokio::spawn(async move {
            let mut con = client.get_async_connection().await?;

            loop {
                let Ok(Some((_, data))) = con
                    .blpop::<_, Option<(String, Vec<u8>)>>(JOBS_KEY, 0)
                    .await
                    else {
                    continue;
                };

                let job: Job<D> = bincode::deserialize(&data)?;
                let job_id = job.id;

                con.set_ex(
                    format!("job:{job_id}"),
                    bincode::serialize(&JobStatus::InProgress)?,
                    STATUS_EXPIRE_SECONDS,
                )
                .await?;

                match f(job, ctx.clone()).await {
                    Ok(_) => {
                        con.set_ex(
                            format!("job:{job_id}"),
                            bincode::serialize(&JobStatus::Completed)?,
                            STATUS_EXPIRE_SECONDS,
                        )
                        .await?;
                        tracing::info!("Job {} done", job_id);
                    }
                    Err(e) => {
                        con.set_ex(
                            format!("job:{job_id}"),
                            bincode::serialize(&JobStatus::Failed)?,
                            STATUS_EXPIRE_SECONDS,
                        )
                        .await?;
                        tracing::error!("Job {job_id} failed: {e}");
                    }
                }
            }
        });

        Ok(handle)
    }

    pub async fn push(&self, msg: D) -> Result<Uuid> {
        let mut con = self.client.get_async_connection().await?;
        let job_id = Uuid::new_v4();
        let job = Job {
            id: job_id,
            data: msg,
        };

        let data = bincode::serialize(&job)?;
        con.rpush(JOBS_KEY, &[data]).await?;

        con.set_ex(
            format!("job:{job_id}"),
            bincode::serialize(&JobStatus::Pending)?,
            STATUS_EXPIRE_SECONDS,
        )
        .await?;

        Ok(job_id)
    }

    pub async fn job_status(&self, job_id: Uuid) -> Result<Option<JobStatus>> {
        let mut con = self.client.get_async_connection().await?;
        let status: Option<Vec<u8>> = con.get(format!("job:{job_id}")).await?;

        match status {
            Some(status) => Ok(Some(bincode::deserialize(&status)?)),
            None => Ok(None),
        }
    }

    pub async fn set_extra<E: Serialize>(&self, job_id: Uuid, extra: E) -> Result<()> {
        let mut con = self.client.get_async_connection().await?;
        let status: Option<Vec<u8>> = con.get(format!("job:{job_id}")).await?;

        if status.is_none() {
            anyhow::bail!("Job not found")
        }

        con.set_ex(
            format!("job:{job_id}:extra"),
            bincode::serialize(&extra)?,
            STATUS_EXPIRE_SECONDS,
        )
        .await?;

        Ok(())
    }

    pub async fn get_extra<E: DeserializeOwned>(&self, job_id: Uuid) -> Result<Option<E>> {
        let mut con = self.client.get_async_connection().await?;
        let extra: Option<Vec<u8>> = con.get(format!("job:{job_id}:extra")).await?;

        match extra {
            Some(extra) => Ok(Some(bincode::deserialize(&extra)?)),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[tokio::test]
    #[ignore]
    async fn test_job_queue() -> Result<()> {
        let ctx = Arc::new(1u32);

        let worker = JobQueue::new("redis://localhost:6379").unwrap();

        let handle = worker
            .start(ctx, |data, ctx| {
                println!("Got job: {:?}, ctx: {}", data, ctx);

                async { Ok(()) }
            })
            .unwrap();

        let job_id = worker.push("hello".to_string()).await.unwrap();
        let job_id = worker.push("world".to_string()).await.unwrap();

        handle.await??;

        Ok(())
    }
}
