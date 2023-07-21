use std::{future::Future, sync::Arc};

use anyhow::Result;
use redis::{AsyncCommands, Client};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tokio::task::JoinHandle;

const STATUS_EXPIRE_SECONDS: usize = 60 * 60 * 24 * 7; // 1 week

// TODO: Implement a proper job queue/explore limitations of this particular design.
//       Also, redis or rabbitmq? Redis is not used for anything else in the project, so rabbitmq
//       might be preferable.

pub type JobId = u64;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
    // Cancelled,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Job<D> {
    pub id: JobId,
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
                    .blpop::<_, Option<(String, Vec<u8>)>>("jobs", 0)
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

    pub async fn push(&self, msg: D) -> Result<JobId> {
        let mut con = self.client.get_async_connection().await?;

        let job_id = con.incr("job_counter", 1).await?;

        let job = Job {
            id: job_id,
            data: msg,
        };

        let data = bincode::serialize(&job)?;
        con.rpush("jobs", &[data]).await?;

        con.set_ex(
            format!("job:{job_id}"),
            bincode::serialize(&JobStatus::Pending)?,
            STATUS_EXPIRE_SECONDS,
        )
        .await?;

        tracing::debug!("New job {}", job_id);

        Ok(job_id)
    }

    pub async fn wait(&self, job_id: JobId) -> Result<()> {
        let mut con = self.client.get_async_connection().await?;

        loop {
            let status: Option<Vec<u8>> = con.get(format!("job:{job_id}")).await?;

            match status {
                Some(status) => {
                    let status: JobStatus = bincode::deserialize(&status)?;
                    match status {
                        JobStatus::Completed => return Ok(()),
                        JobStatus::Failed => anyhow::bail!("Job failed"),
                        JobStatus::Pending | JobStatus::InProgress => {
                            // TODO: use pub/sub?
                            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                            continue;
                        }
                    }
                }
                None => anyhow::bail!("Job not found"),
            }
        }
    }

    pub async fn job_status(&self, job_id: JobId) -> Result<Option<JobStatus>> {
        let mut con = self.client.get_async_connection().await?;
        let status: Option<Vec<u8>> = con.get(format!("job:{job_id}")).await?;

        match status {
            Some(status) => Ok(Some(bincode::deserialize(&status)?)),
            None => Ok(None),
        }
    }

    pub async fn add_job_mapping<T: ToString>(&self, job_id: JobId, key: T) -> Result<()> {
        let mut con = self.client.get_async_connection().await?;
        let key = key.to_string();

        con.set_ex(
            format!("job_mapping:{key}"),
            bincode::serialize(&job_id)?,
            STATUS_EXPIRE_SECONDS,
        )
        .await?;

        Ok(())
    }

    pub async fn get_job_mapping<T: ToString>(&self, key: T) -> Result<Option<JobId>> {
        let mut con = self.client.get_async_connection().await?;
        let key = key.to_string();

        let job_id: Option<Vec<u8>> = con.get(format!("job_mapping:{key}")).await?;

        match job_id {
            Some(job_id) => Ok(Some(bincode::deserialize(&job_id)?)),
            None => Ok(None),
        }
    }

    pub async fn cancel_jobs_after(&self, job_id: JobId) -> Result<()> {
        let mut con = self.client.get_async_connection().await?;

        let job_ids: Vec<JobId> = con
            .lrange::<_, Vec<Vec<u8>>>("jobs", 0, -1)
            .await?
            .into_iter()
            .map(|data| bincode::deserialize(&data).map_err(Into::into))
            .collect::<Result<_>>()?;

        for id in job_ids {
            if id > job_id {
                con.set_ex(
                    format!("job:{id}"),
                    bincode::serialize(&JobStatus::Failed)?,
                    STATUS_EXPIRE_SECONDS,
                )
                .await?;
            }
        }

        Ok(())
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

        let _job_id = worker.push("hello".to_string()).await.unwrap();
        let _job_id = worker.push("world".to_string()).await.unwrap();

        handle.await??;

        Ok(())
    }
}
