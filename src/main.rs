use std::{
    fs::create_dir_all,
    sync::{Arc, Barrier},
    time::Duration,
};

use anyhow::{bail, Context, Result};
use opentelemetry::{
    global,
    sdk::{
        export::metrics::aggregation::cumulative_temporality_selector, metrics::selectors, trace,
    },
    Context as OpenTelemetryCtx,
};
use opentelemetry_otlp::{ExportConfig, Protocol, WithExportConfig};
use tokio::{
    runtime,
    sync::{mpsc, oneshot},
    task::spawn_blocking,
    time::sleep,
};
use tokio_graceful_shutdown::{errors::SubsystemError, Toplevel};
use tracing::{error, info, warn};
use tracing_subscriber::{filter::LevelFilter, prelude::__tracing_subscriber_SubscriberExt, Layer};

use crate::{conf::SeeleWorkMode, shared::metrics::METRICS_RESOURCE, worker::action};

mod cgroup;
mod composer;
mod conf;
mod entities;
mod exchange;
mod healthz;
mod shared;
mod worker;

fn main() {
    let runtime = runtime::Builder::new_current_thread()
        .max_blocking_threads(conf::CONFIG.thread_counts.worker + conf::CONFIG.thread_counts.runner)
        .thread_keep_alive(Duration::from_secs(u64::MAX))
        .enable_all()
        .build()
        .expect("Error building tokio runtime");
    runtime
        .block_on(async move {
            {
                spawn_blocking(|| -> Result<()> {
                    cgroup::check_cgroup_setup().context("Error checking cgroup setup")?;
                    cgroup::initialize_cgroup_subtrees()
                        .context("Error initializing cgroup subtrees")?;
                    anyhow::Ok(())
                })
                .await??;

                let count = conf::CONFIG.thread_counts.worker + conf::CONFIG.thread_counts.runner;
                let begin_barrier = Arc::new(Barrier::new(count));
                let end_barrier = Arc::new(Barrier::new(count));

                for _ in 0..(count - 1) {
                    let begin_barrier = begin_barrier.clone();
                    let end_barrier = end_barrier.clone();
                    spawn_blocking(move || {
                        begin_barrier.wait();
                        end_barrier.wait();
                    });
                }

                spawn_blocking(move || {
                    begin_barrier.wait();
                    let result = cgroup::bind_application_threads();
                    end_barrier.wait();
                    result
                })
                .await?
                .context("Error binding application threads")?;
            }

            match &conf::CONFIG.telemetry {
                None => {
                    tracing::subscriber::set_global_default(
                        tracing_subscriber::fmt()
                            .compact()
                            .with_line_number(true)
                            .with_max_level(conf::CONFIG.log_level)
                            .finish(),
                    )
                    .context("Failed to initialize the tracing subscriber")?;
                }
                Some(telemetry) => {
                    info!("Initializing telemetry");

                    let tracer = opentelemetry_otlp::new_pipeline()
                        .tracing()
                        .with_exporter(
                            opentelemetry_otlp::new_exporter().tonic().with_export_config(
                                ExportConfig {
                                    endpoint: telemetry.collector_url.clone(),
                                    timeout: Duration::from_secs(5),
                                    protocol: Protocol::Grpc,
                                },
                            ),
                        )
                        .with_trace_config(trace::config().with_resource(METRICS_RESOURCE.clone()))
                        .install_batch(opentelemetry::runtime::Tokio)
                        .context("Failed to initialize the tracer")?;

                    let metrics = opentelemetry_otlp::new_pipeline()
                        .metrics(
                            selectors::simple::histogram(vec![5.0, 15.0, 30.0, 60.0]),
                            cumulative_temporality_selector(),
                            opentelemetry::runtime::Tokio,
                        )
                        .with_exporter(
                            opentelemetry_otlp::new_exporter().tonic().with_export_config(
                                ExportConfig {
                                    endpoint: telemetry.collector_url.clone(),
                                    timeout: Duration::from_secs(5),
                                    protocol: Protocol::Grpc,
                                },
                            ),
                        )
                        .with_resource(METRICS_RESOURCE.clone())
                        .with_period(Duration::from_secs(5))
                        .build()
                        .context("Failed to initialize the metrics")?;

                    _ = shared::metrics::METRICS_CONTROLLER.set(metrics);

                    tracing::subscriber::set_global_default(
                        tracing_subscriber::registry()
                            .with(
                                tracing_subscriber::fmt::layer()
                                    .compact()
                                    .with_line_number(true)
                                    .with_filter::<LevelFilter>(conf::CONFIG.log_level.into()),
                            )
                            .with(
                                tracing_opentelemetry::layer()
                                    .with_tracer(tracer)
                                    .with_filter(LevelFilter::INFO),
                            ),
                    )
                    .context("Failed to initialize the tracing subscriber")?;
                }
            }

            spawn_blocking(|| -> Result<()> {
                info!("Checking cpu counts");
                let logical_cpu_count = num_cpus::get();
                let physical_cpu_count = num_cpus::get_physical();
                if physical_cpu_count < logical_cpu_count {
                    // TODO: Add link to document
                    warn!(
                        "Seele does not recommand enabling the cpu's SMT technology, current \
                         logical cpu count: {}, physical cpu count: {}",
                        logical_cpu_count, physical_cpu_count
                    )
                }

                if !matches!(&conf::CONFIG.work_mode, SeeleWorkMode::RootlessContainerized) {
                    info!("Checking id maps");
                    if action::run_container::SUBUIDS.count < 65536 {
                        bail!(
                            "The user specified in the run_container namespace config has not \
                             enough subuid mapping range"
                        );
                    }
                    if action::run_container::SUBGIDS.count < 65536 {
                        bail!(
                            "The group specified in the run_container namespace config has not \
                             enough subgid mapping range"
                        );
                    }
                }

                info!("Creating necessary directories in {}", conf::PATHS.root.display());
                for path in [
                    &conf::PATHS.images,
                    &conf::PATHS.submissions,
                    &conf::PATHS.evicted,
                    &conf::PATHS.temp,
                ] {
                    create_dir_all(path).with_context(|| {
                        format!("Error creating the directory: {}", path.display())
                    })?;
                }

                info!("Registering metrics");
                shared::metrics::register_gauge_metrics()
                    .context("Error registering gauge metrics")?;

                Ok(())
            })
            .await??;

            let result = Toplevel::new()
                .start("seele", |handle| async move {
                    info!("Worker started bootstrap");
                    let (tx, rx) = oneshot::channel();
                    handle.start("bootstrap", |handle| worker::worker_bootstrap(handle, tx));

                    _ = rx.await;
                    if handle.is_shutdown_requested() {
                        return anyhow::Ok(());
                    }

                    info!("Initializing seele components");
                    let (composer_queue_tx, composer_queue_rx) =
                        mpsc::channel(conf::CONFIG.thread_counts.runner);
                    let (worker_queue_tx, worker_queue_rx) =
                        mpsc::channel(conf::CONFIG.thread_counts.runner * 4);
                    handle.start("healthz", |handle| healthz::healthz_main(handle));
                    handle.start("exchange", |handle| {
                        exchange::exchange_main(handle, composer_queue_tx)
                    });
                    handle.start("composer", |handle| {
                        composer::composer_main(handle, composer_queue_rx, worker_queue_tx)
                    });
                    handle.start("worker", |handle| worker::worker_main(handle, worker_queue_rx));

                    handle.on_shutdown_requested().await;
                    anyhow::Ok(())
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
                        SubsystemError::Cancelled(name) => {
                            error!("Subsystem '{}' was cancelled", name);
                        }
                        SubsystemError::Panicked(name) => {
                            error!("Subsystem '{}' panicked", name);
                        }
                    }
                }
            }

            if conf::CONFIG.telemetry.is_some() {
                _ = spawn_blocking(|| {
                    global::shutdown_tracer_provider();
                    _ = shared::metrics::METRICS_CONTROLLER
                        .get()
                        .unwrap()
                        .stop(&OpenTelemetryCtx::current());
                })
                .await;
            }

            sleep(Duration::from_secs(3)).await;

            anyhow::Ok(())
        })
        .unwrap();
}
