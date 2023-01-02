use crate::conf::ExchangeConfig;
use crate::exchange::http::run_http_exchange;
use crate::{composer::ComposerQueueItem, conf};
use tokio::sync::mpsc;
use tokio_graceful_shutdown::SubsystemHandle;
use tracing::info;

mod http;

pub async fn exchange_main(
    handle: SubsystemHandle,
    composer_queue_tx: mpsc::Sender<ComposerQueueItem>,
) -> anyhow::Result<()> {
    info!("Initializing exchanges based on the configuration");

    for exchange in &conf::CONFIG.exchange {
        match exchange {
            ExchangeConfig::Http { address, port } => {
                let tx = composer_queue_tx.clone();
                handle.start(&format!("http-{}-{}", address, port), move |handle| {
                    run_http_exchange(handle, tx.clone(), *address, *port)
                });
            }
        }
    }

    handle.on_shutdown_requested().await;
    Ok(())
}
