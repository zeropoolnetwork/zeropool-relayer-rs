mod backend;
mod config;
mod indexer;
mod json_api;

#[cfg(not(feature = "near-indexer-framework"))]
#[tokio::main]
async fn main() {
    start().await;
}

#[cfg(feature = "near-indexer-framework")]
#[actix::main]
async fn main() {
    start().await;
}

async fn start() {
    dotenv::dotenv().ok();
    tracing_subscriber::fmt::init();

    let config = config::Config::init().unwrap();

    tracing::info!("{config:#?}");

    let (storage, indexer_worker, storage_worker) =
        indexer::start_indexer(config.clone()).await.unwrap();

    let json_api = tokio::spawn(json_api::start(config.port, storage));

    tokio::select! {
        res = indexer_worker => {
            tracing::error!("Indexer worker stopped: {:?}", res);
        }
        res = storage_worker => {
            tracing::error!("Storage worker exited unexpectedly: {:?}", res);
        }
        res = json_api => {
            tracing::error!("JSON API exited unexpectedly: {:?}", res);
        }
    }
}
