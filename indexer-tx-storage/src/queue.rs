use std::{future::Future, sync::Arc};

use anyhow::Result;
use redis::{AsyncCommands, Client};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tokio::task::JoinHandle;
use uuid::Uuid;

const KEY: &str = "txs";

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

        let data = bincode::serialize(&msg)?;
        con.rpush(KEY, &[data]).await?;
        
        Ok(job_id)
    }
}
