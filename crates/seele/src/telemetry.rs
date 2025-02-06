use std::time::Duration;

use anyhow::{Context, Result};
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::{ExportConfig, MetricExporter, Protocol, SpanExporter, WithExportConfig};
use opentelemetry_sdk::{
    metrics::{PeriodicReader, SdkMeterProvider, Temporality},
    trace::TracerProvider,
};
use tracing::*;
use tracing_subscriber::{Layer, filter::LevelFilter, prelude::*};

use crate::{conf, shared};

pub async fn setup_telemetry() -> Result<()> {
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
        .with_resource(shared::metrics::metrics_resource())
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

    let meter_provider = SdkMeterProvider::builder()
        .with_reader(
            PeriodicReader::builder(metric_exporter, opentelemetry_sdk::runtime::Tokio)
                .with_interval(Duration::from_secs(3))
                .build(),
        )
        .with_resource(shared::metrics::metrics_resource())
        .build();

    shared::metrics::init_with_meter_provider(meter_provider);

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
