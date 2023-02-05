use anyhow::Result;
use tokio::sync::mpsc;
use tokio_graceful_shutdown::SubsystemHandle;
use tracing::info;

use crate::{
    composer::ComposerQueueItem, conf, conf::ExchangeConfig, exchange::http::run_http_exchange,
};

mod http;

pub async fn exchange_main(
    handle: SubsystemHandle,
    composer_queue_tx: mpsc::Sender<ComposerQueueItem>,
) -> Result<()> {
    info!("Initializing exchanges based on the configuration");

    for exchange in &conf::CONFIG.exchange {
        match exchange {
            ExchangeConfig::Http(config) => {
                let tx = composer_queue_tx.clone();
                handle.start(&format!("http-{}-{}", config.address, config.port), move |handle| {
                    run_http_exchange(handle, tx.clone(), config)
                });
            }
        }
    }

    handle.on_shutdown_requested().await;
    Ok(())
}
