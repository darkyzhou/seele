#![allow(dead_code)]

use std::{
    fs::create_dir_all,
    sync::{Arc, Barrier},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use opentelemetry::{global, trace::TracerProvider as _};
use opentelemetry_otlp::{ExportConfig, MetricExporter, Protocol, SpanExporter, WithExportConfig};
use opentelemetry_sdk::{
    metrics::{PeriodicReader, SdkMeterProvider, Temporality},
    trace::TracerProvider,
};
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
use tracing_subscriber::{Layer, filter::LevelFilter, prelude::*};

use crate::{conf::SeeleWorkMode, worker::action};

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
            setup_cgroup().await?;

            setup_telemetry().await?;

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
                    shared::metrics::METRICS_PROVIDER.get().unwrap().shutdown().ok();
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

async fn setup_cgroup() -> Result<()> {
    spawn_blocking(|| -> Result<()> {
        cgroup::check_cgroup_setup().context("Error checking cgroup setup")?;
        cgroup::initialize_cgroup_subtrees().context("Error initializing cgroup subtrees")
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
    .context("Error binding application threads")
}

async fn setup_telemetry() -> Result<()> {
    if conf::CONFIG.telemetry.is_none() {
        tracing::subscriber::set_global_default(
            tracing_subscriber::fmt()
                .compact()
                .with_line_number(true)
                .with_max_level(conf::CONFIG.log_level)
                .finish(),
        )
        .context("Failed to initialize the tracing subscriber")?;
    }

    let telemetry = conf::CONFIG.telemetry.as_ref().unwrap();

    info!("Initializing telemetry");

    let span_exporter = SpanExporter::builder()
        .with_tonic()
        .with_export_config(ExportConfig {
            endpoint: Some(telemetry.collector_url.clone()),
            timeout: Duration::from_secs(5),
            protocol: Protocol::Grpc,
        })
        .build()
        .context("Failed to initialize the tracer")?;

    let tracer_provider = TracerProvider::builder()
        .with_batch_exporter(span_exporter, opentelemetry_sdk::runtime::Tokio)
        .with_resource(shared::metrics::METRICS_RESOURCE.clone())
        .build();

    let tracer = tracer_provider.tracer("seele");

    let metric_exporter = MetricExporter::builder()
        .with_temporality(Temporality::Cumulative)
        .with_tonic()
        .with_export_config(ExportConfig {
            endpoint: Some(telemetry.collector_url.clone()),
            timeout: Duration::from_secs(5),
            protocol: Protocol::Grpc,
        })
        .build()
        .context("Failed to initialize the metrics")?;

    let metrics = SdkMeterProvider::builder()
        .with_reader(
            PeriodicReader::builder(metric_exporter, opentelemetry_sdk::runtime::Tokio)
                .with_interval(Duration::from_secs(3))
                .build(),
        )
        .with_resource(shared::metrics::METRICS_RESOURCE.clone())
        .build();

    shared::metrics::METRICS_PROVIDER.set(metrics).ok();

    tracing::subscriber::set_global_default(
        tracing_subscriber::registry()
            .with(
                tracing_subscriber::fmt::layer()
                    .compact()
                    .with_line_number(true)
                    .with_filter::<LevelFilter>(conf::CONFIG.log_level.into()),
            )
            .with(
                tracing_opentelemetry::layer().with_tracer(tracer).with_filter(LevelFilter::INFO),
            ),
    )
    .context("Failed to initialize the tracing subscriber")
}

fn check_env() -> Result<()> {
    debug!("Checking cpu counts");
    let logical_cpu_count = num_cpus::get();
    let physical_cpu_count = num_cpus::get_physical();

    info!("CPU counts: {}/{} (logical/physical)", logical_cpu_count, physical_cpu_count);

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
