use std::{
    fs::create_dir_all,
    process,
    sync::{Arc, Barrier},
    time::Duration,
};

use tokio::{runtime, sync::mpsc, task::spawn_blocking, time};
use tokio_graceful_shutdown::{errors::SubsystemError, Toplevel};
use tracing::{error, info};

use crate::worker::run_container;

mod cgroup;
mod composer;
mod conf;
mod entities;
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

    info!("Checking cgroup setup");
    cgroup::check_cgroup_setup().expect("Error checking cgroup setup");

    info!("Initializing cgroup subtrees");
    cgroup::initialize_cgroup_subtrees().expect("Error initializing cgroup subtrees");

    info!("Creating necessary directories in {}", conf::PATHS.root.display());
    {
        for path in [
            &conf::PATHS.images,
            &conf::PATHS.submissions,
            &conf::PATHS.evicted,
            &conf::PATHS.states,
            &conf::PATHS.temp_mounts,
        ] {
            create_dir_all(path)
                .expect(&format!("Error creating the directory: {}", path.display()));
        }
    }

    let pid = process::id();

    info!("Initializing the runtime");
    let runtime = runtime::Builder::new_multi_thread()
        .worker_threads(conf::CONFIG.worker_thread_count)
        .max_blocking_threads(conf::CONFIG.blocking_thread_count)
        .thread_keep_alive(Duration::from_secs(u64::MAX))
        .enable_all()
        .build()
        .unwrap();
    runtime
        .block_on(async move {
            {
                run_container::init_runner_pool().await;
                time::sleep(Duration::from_millis(300)).await;

                let begin_barrier = Arc::new(Barrier::new(conf::CONFIG.blocking_thread_count));
                let end_barrier = Arc::new(Barrier::new(conf::CONFIG.blocking_thread_count));
                for _ in 0..(conf::CONFIG.blocking_thread_count - 1) {
                    let begin_barrier = begin_barrier.clone();
                    let end_barrier = end_barrier.clone();
                    spawn_blocking(move || {
                        begin_barrier.wait();
                        end_barrier.wait();
                    });
                }

                spawn_blocking(move || {
                    begin_barrier.wait();
                    let result = cgroup::bind_app_threads(pid);
                    end_barrier.wait();
                    result
                })
                .await
                .unwrap()
                .expect("Error binding app threads");
            }

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
                            error!("Subsystem '{}' failed: {:?}", name, err);
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

            anyhow::Ok(())
        })
        .expect("Error initializing runtime");
}
