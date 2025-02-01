use std::{
    borrow::Cow,
    sync::{LazyLock, OnceLock, atomic::Ordering},
};

use opentelemetry::{
    InstrumentationScope, KeyValue, global,
    metrics::{Histogram, Meter, ObservableGauge},
};
use opentelemetry_sdk::{Resource, metrics::SdkMeterProvider};

use super::runner;
use crate::conf;

pub static METRICS_RESOURCE: LazyLock<Resource> = LazyLock::new(|| {
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

    Resource::new(pairs)
});

pub static METRICS_PROVIDER: OnceLock<SdkMeterProvider> = OnceLock::new();

pub static METER: LazyLock<Meter> = LazyLock::new(|| {
    let scope = InstrumentationScope::builder("seele")
        .with_version(Cow::Borrowed(env!("CARGO_PKG_VERSION")))
        .build();

    global::meter_with_scope(scope)
});

pub static SUBMISSION_HANDLING_HISTOGRAM: LazyLock<Histogram<f64>> = LazyLock::new(|| {
    METER
        .f64_histogram("seele.submission.duration")
        .with_description("Duration of submissions handling")
        .with_unit("s")
        .build()
});

pub static RUNNER_COUNT_GAUGE: LazyLock<ObservableGauge<u64>> = LazyLock::new(|| {
    METER
        .u64_observable_gauge("seele.runner.count")
        .with_description("Count of available runner threads")
        .with_callback(|ctx| {
            ctx.observe(conf::CONFIG.thread_counts.runner as u64, &[]);
        })
        .build()
});

pub static PENDING_CONTAINER_ACTION_COUNT_GAUGE: LazyLock<ObservableGauge<u64>> =
    LazyLock::new(|| {
        METER
            .u64_observable_gauge("seele.action.container.pending.count")
            .with_description("Count of pending container actions in the worker queue")
            .with_callback(|ctx| ctx.observe(runner::PENDING_TASKS.load(Ordering::SeqCst), &[]))
            .build()
    });
