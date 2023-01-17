use std::time::Duration;
use tokio::{runtime, sync::mpsc};
use tokio_graceful_shutdown::{errors::SubsystemError, Toplevel};
use tracing::{error, info};

mod composer;
mod conf;
mod entity;
mod exchange;
mod shared;
mod worker;

fn main() {
    tracing::subscriber::set_global_default(
        tracing_subscriber::fmt()
            .compact()
            .with_line_number(true)
            .with_max_level(tracing::Level::DEBUG)
            .finish(),
    )
    .expect("Failed to initialize the logger");

    info!(root = %conf::PATHS.root.display(), "Creating necessary directories");
    std::fs::create_dir_all(&conf::PATHS.images).unwrap();
    std::fs::create_dir_all(&conf::PATHS.submissions).unwrap();
    std::fs::create_dir_all(&conf::PATHS.evicted).unwrap();
    std::fs::create_dir_all(&conf::PATHS.states).unwrap();

    info!("Creating runtime");
    let runtime = runtime::Builder::new_multi_thread()
        .worker_threads(3)
        .enable_all()
        .max_blocking_threads(1)
        .build()
        .unwrap();
    runtime.block_on(async {
        let (composer_queue_tx, composer_queue_rx) =
            mpsc::channel::<composer::ComposerQueueItem>(16);
        let (worker_queue_tx, worker_queue_rx) =
            async_channel::bounded::<worker::WorkerQueueItem>(16);

        let result = Toplevel::new()
            .start("exchange", |handle| exchange::exchange_main(handle, composer_queue_tx))
            .start("composer", |handle| {
                composer::composer_main(handle, composer_queue_rx, worker_queue_tx)
            })
            .start("worker", |handle| worker::worker_main(handle, worker_queue_rx))
            .catch_signals()
            .handle_shutdown_requests(Duration::from_secs(10))
            .await;
        if let Err(err) = result {
            error!("Seele encountered fatal issue(s):");
            for error in err.get_subsystem_errors() {
                match error {
                    SubsystemError::Failed(name, err) => {
                        error!("Subsystem '{}' failed: {:#}", name, err);
                    }
                    SubsystemError::Cancelled(name) => {
                        error!("Subsystem '{}' was cancelled", name)
                    }
                    SubsystemError::Panicked(name) => {
                        error!("Subsystem '{}' panicked", name)
                    }
                }
            }
        }
    });
}
