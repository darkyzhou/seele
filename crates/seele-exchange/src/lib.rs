use anyhow::Result;
use seele_composer::{self as composer, ComposerQueueItem};
use seele_config::{self as conf, ExchangeConfig};
use tokio::sync::mpsc;
use tokio_graceful_shutdown::{SubsystemBuilder, SubsystemHandle};
use tracing::info;

pub use self::amqp::is_amqp_healthy;

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
                handle.start(SubsystemBuilder::new(format!("{name}-http"), move |handle| {
                    http::run(name, handle, tx, config)
                }));
            }
            ExchangeConfig::Amqp(config) => {
                let tx = composer_queue_tx.clone();
                handle.start(SubsystemBuilder::new(format!("{name}-amqp"), move |handle| {
                    amqp::run(name, handle, tx, config)
                }));
            }
        }
    }

    handle.on_shutdown_requested().await;
    Ok(())
}
