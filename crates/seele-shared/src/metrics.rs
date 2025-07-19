use std::{
    borrow::Cow,
    sync::{LazyLock, OnceLock, atomic::Ordering},
};

use opentelemetry::{
    InstrumentationScope, KeyValue, global,
    metrics::{Histogram, Meter},
};
use opentelemetry_sdk::{Resource, metrics::SdkMeterProvider};

use super::runner;
use crate::conf;

static METRICS_RESOURCE: LazyLock<Resource> = LazyLock::new(|| {
    let mut pairs = vec![
        KeyValue::new("service.name", "seele"),
        KeyValue::new(
            "service.version",
            conf::env::COMMIT_TAG.or(*conf::env::COMMIT_SHA).unwrap_or("unknown"),
        ),
        KeyValue::new(
            "service.instance.id",
            format!("{}-{}", *conf::HOSTNAME, nano_id::base62::<8>()),
        ),
        KeyValue::new("host.name", conf::HOSTNAME.clone()),
    ];

    if let Some(container_name) = conf::CONTAINER_NAME.as_ref() {
        pairs.push(KeyValue::new("container.name", container_name.clone()));
    }

    if let Some(container_image_name) = conf::CONTAINER_IMAGE_NAME.as_ref() {
        pairs.push(KeyValue::new("container.image.name", container_image_name.clone()));
    }

    if let Some(tag) = conf::COMMIT_TAG.as_ref() {
        pairs.push(KeyValue::new("commit.tag", *tag));
    }

    if let Some(sha) = conf::COMMIT_SHA.as_ref() {
        pairs.push(KeyValue::new("commit.sha", *sha));
    }

    Resource::builder().with_attributes(pairs).build()
});

/// Get the global metrics resource
#[inline]
pub fn metrics_resource() -> Resource {
    METRICS_RESOURCE.clone()
}

static METER_PROVIDER: OnceLock<SdkMeterProvider> = OnceLock::new();

static SEELE_METER: LazyLock<Meter> = LazyLock::new(|| {
    let scope = InstrumentationScope::builder("seele")
        .with_version(Cow::Borrowed(env!("CARGO_PKG_VERSION")))
        .build();

    global::meter_with_scope(scope)
});

/// Initialize the global meter provider
///
/// The `SdkMeterProvider` is `Arc<SdkMeterProviderInner>`
///
/// We have two things want to do:
///
///   - Set the global meter provider via `global::set_meter_provider`
///   - Maually shutdown the meter provider on the end of the program, because
///     we cannot manually shutdown via `GlobalMeterProvider trait`
pub fn init_with_meter_provider(provider: SdkMeterProvider) {
    if METER_PROVIDER.set(provider.clone()).is_err() {
        panic!("Meter provider is already initialized");
    }

    global::set_meter_provider(provider);

    register_observable_gauges();
}

/// Shutdown the global meter provider
#[inline]
pub fn shutdown_meter_provider() {
    if let Some(provider) = METER_PROVIDER.get() {
        provider.shutdown().ok();
    }
}

/// Register observable gauges
///
/// These metrics are registered with callbacks to update their values
#[inline]
fn register_observable_gauges() {
    SEELE_METER
        .u64_observable_gauge("seele.runner.count")
        .with_description("Count of available runner threads")
        .with_callback(|observer| {
            observer.observe(conf::CONFIG.thread_counts.runner as u64, &[]);
        })
        .build();

    SEELE_METER
        .u64_observable_gauge("seele.action.container.pending.count")
        .with_description("Count of pending container actions in the worker queue")
        .with_callback(|observer| {
            observer.observe(runner::PENDING_TASKS.load(Ordering::SeqCst), &[])
        })
        .build();
}

static SUBMISSION_HANDLING_HISTOGRAM: LazyLock<Histogram<f64>> = LazyLock::new(|| {
    SEELE_METER
        .f64_histogram("seele.submission.duration")
        .with_description("Duration of submissions handling")
        .with_unit("s")
        .build()
});

/// Record the submission handling duration
#[inline]
pub fn record_submission_handling_duration(duration: f64, attrs: &[KeyValue]) {
    SUBMISSION_HANDLING_HISTOGRAM.record(duration, attrs);
}
