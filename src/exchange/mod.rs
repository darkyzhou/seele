use anyhow::Result;
use tokio::sync::mpsc;
use tokio_graceful_shutdown::SubsystemHandle;
use tracing::info;

use crate::{composer::ComposerQueueItem, conf, conf::ExchangeConfig};

mod amqp;
mod http;

pub async fn exchange_main(
    handle: SubsystemHandle,
    composer_queue_tx: mpsc::Sender<ComposerQueueItem>,
) -> Result<()> {
    info!("Initializing exchanges based on the configuration");

    for (index, exchange) in conf::CONFIG.exchange.iter().enumerate() {
        match exchange {
            ExchangeConfig::Http(config) => {
                let tx = composer_queue_tx.clone();
                handle.start(&format!("http-{index}"), move |handle| http::run(handle, tx, config));
            }
            ExchangeConfig::Amqp(config) => {
                let tx = composer_queue_tx.clone();
                handle.start(&format!("amqp-{index}"), move |handle| amqp::run(handle, tx, config));
            }
        }
    }

    handle.on_shutdown_requested().await;
    Ok(())
}
