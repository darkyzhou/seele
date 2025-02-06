#![allow(dead_code)]

use std::{fs::create_dir_all, time::Duration};

use anyhow::{Context, Result, bail};
use opentelemetry::global;
use seele_composer as composer;
use seele_config as conf;
use seele_exchange as exchange;
use seele_shared as shared;
use seele_worker as worker;
use tokio::{
    runtime,
    sync::{mpsc, oneshot},
    task::spawn_blocking,
    time::sleep,
};
use tokio_graceful_shutdown::{
    SubsystemBuilder, SubsystemHandle, Toplevel, errors::SubsystemError,
};
use tracing::*;

use crate::{conf::SeeleWorkMode, worker::action};

mod cgroup;
mod healthz;
mod telemetry;

fn main() {
    let runtime = runtime::Builder::new_current_thread()
        .max_blocking_threads(conf::CONFIG.thread_counts.worker + conf::CONFIG.thread_counts.runner)
        .thread_keep_alive(Duration::from_secs(u64::MAX))
        .enable_all()
        .build()
        .expect("Error building tokio runtime");

    runtime
        .block_on(async move {
            telemetry::setup_telemetry().await?;

            cgroup::setup_cgroup().await?;

            spawn_blocking(check_env).await??;

            let result = Toplevel::new(move |s| async move {
                s.start(SubsystemBuilder::new("seele", seele));
            })
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
                        SubsystemError::Panicked(name) => {
                            error!("Subsystem '{}' panicked", name);
                        }
                    }
                }
            }

            if conf::CONFIG.telemetry.is_some() {
                spawn_blocking(|| {
                    global::shutdown_tracer_provider();
                    shared::metrics::shutdown_meter_provider();
                })
                .await
                .ok();
            }

            sleep(Duration::from_secs(3)).await;

            anyhow::Ok(())
        })
        .unwrap();
}

async fn seele(handle: SubsystemHandle) -> Result<()> {
    let (tx, rx) = oneshot::channel();

    info!("Worker started bootstrap");

    handle.start(SubsystemBuilder::new("bootstrap", |handle| worker::worker_bootstrap(handle, tx)));

    if !rx.await? {
        handle.on_shutdown_requested().await;
        return anyhow::Ok(());
    }

    info!("Initializing seele components");

    let (composer_queue_tx, composer_queue_rx) = mpsc::channel(conf::CONFIG.thread_counts.runner);
    let (worker_queue_tx, worker_queue_rx) = mpsc::channel(conf::CONFIG.thread_counts.runner * 4);

    handle.start(SubsystemBuilder::new("healthz", healthz::healthz_main));

    handle.start(SubsystemBuilder::new("exchange", |handle| {
        exchange::exchange_main(handle, composer_queue_tx)
    }));

    handle.start(SubsystemBuilder::new("composer", |handle| {
        composer::composer_main(handle, composer_queue_rx, worker_queue_tx)
    }));

    handle.start(SubsystemBuilder::new("worker", |handle| {
        worker::worker_main(handle, worker_queue_rx)
    }));

    handle.on_shutdown_requested().await;
    anyhow::Ok(())
}

fn check_env() -> Result<()> {
    debug!("Checking cpu counts");
    let logical_cpu_count = num_cpus::get();
    let physical_cpu_count = num_cpus::get_physical();

    if physical_cpu_count < logical_cpu_count {
        // TODO: Add link to document
        warn!(
            "Seele does not recommend enabling the cpu's SMT technology, current logical cpu \
             count: {}, physical cpu count: {}",
            logical_cpu_count, physical_cpu_count
        )
    }

    if !matches!(&conf::CONFIG.work_mode, SeeleWorkMode::RootlessContainerized) {
        debug!("Checking id maps");
        if action::run_container::SUBUIDS.count < 65536 {
            bail!(
                "The user specified in the run_container namespace config has not enough subuid \
                 mapping range"
            );
        }
        if action::run_container::SUBGIDS.count < 65536 {
            bail!(
                "The group specified in the run_container namespace config has not enough subgid \
                 mapping range"
            );
        }

        info!(
            "User namespace subuid mapping range: {}-{}, subgid mapping range: {}-{}",
            action::run_container::SUBUIDS.begin,
            action::run_container::SUBUIDS.begin + action::run_container::SUBUIDS.count,
            action::run_container::SUBGIDS.begin,
            action::run_container::SUBGIDS.begin + action::run_container::SUBGIDS.count
        );
    }

    info!("Creating necessary directories in {}", conf::PATHS.root.display());
    for path in [&conf::PATHS.images, &conf::PATHS.submissions, &conf::PATHS.temp] {
        create_dir_all(path)
            .with_context(|| format!("Error creating the directory: {}", path.display()))?;
    }

    info!("Registering metrics");

    Ok(())
}
