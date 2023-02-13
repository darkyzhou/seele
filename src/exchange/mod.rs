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

    for (name, exchange) in &conf::CONFIG.exchange {
        match exchange {
            ExchangeConfig::Http(config) => {
                let tx = composer_queue_tx.clone();
                handle.start(&format!("{name}-http"), move |handle| {
                    http::run(name, handle, tx, config)
                });
            }
            ExchangeConfig::Amqp(config) => {
                let tx = composer_queue_tx.clone();
                handle.start(&format!("{name}-amqp"), move |handle| {
                    amqp::run(name, handle, tx, config)
                });
            }
        }
    }

    handle.on_shutdown_requested().await;
    Ok(())
}
