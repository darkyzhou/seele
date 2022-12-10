use std::time::Duration;
use tokio::{
    runtime,
    sync::{broadcast, mpsc},
};
use tokio_graceful_shutdown::{errors::SubsystemError, Toplevel};
use tracing::error;

mod composer;
mod conf;
mod entity;
mod exchange;
mod queues;
mod worker;

fn main() {
    let rt = runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()
        .expect("Failed to create the tokio runtime");

    rt.block_on(async {
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
            error!("Crush encountered fatal issue(s):");
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
