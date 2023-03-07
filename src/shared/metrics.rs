use anyhow::Result;
use once_cell::sync::{Lazy, OnceCell};
use opentelemetry::{
    global,
    metrics::{Histogram, Meter, ObservableGauge, Unit},
    sdk::{metrics::controllers::BasicController, Resource},
    KeyValue,
};

use crate::conf;

pub static METRICS_RESOURCE: Lazy<Resource> = Lazy::new(|| {
    let mut pairs = vec![
        KeyValue::new("service.name", "seele"),
        KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
        KeyValue::new("host.name", conf::HOSTNAME.clone()),
    ];

    if let Some(container_name) = conf::CONTAINER_NAME.as_ref() {
        pairs.push(KeyValue::new("container.name", container_name.clone()));
    }

    if let Some(container_image_name) = conf::CONTAINER_IMAGE_NAME.as_ref() {
        pairs.push(KeyValue::new("container.image.name", container_image_name.clone()));
    }

    if let Some(container_image_tag) = conf::CONTAINER_IMAGE_TAG.as_ref() {
        pairs.push(KeyValue::new("container.image.tag", container_image_tag.clone()));
    }

    Resource::new(pairs)
});

pub static METRICS_CONTROLLER: OnceCell<BasicController> = OnceCell::new();

pub static METER: Lazy<Meter> =
    Lazy::new(|| global::meter_with_version("seele", Some("0.1"), None));

pub static SUBMISSION_HANDLING_HISTOGRAM: Lazy<Histogram<f64>> = Lazy::new(|| {
    METER
        .f64_histogram("seele.submission.duration")
        .with_description("Duration of submissions handling")
        .with_unit(Unit::new("s"))
        .init()
});

pub static RUNNER_COUNT_GAUGE: Lazy<ObservableGauge<u64>> = Lazy::new(|| {
    METER
        .u64_observable_gauge("seele.runner.count")
        .with_description("Count of available runner threads")
        .init()
});

pub static PENDING_SUBMISSION_COUNT_GAUGE: Lazy<ObservableGauge<u64>> = Lazy::new(|| {
    METER
        .u64_observable_gauge("seele.submission.pending.count")
        .with_description("Count of pending submissions in the composer queue")
        .init()
});

pub static PENDING_ACTION_COUNT_GAUGE: Lazy<ObservableGauge<u64>> = Lazy::new(|| {
    METER
        .u64_observable_gauge("seele.action.pending.count")
        .with_description("Count of pending actions in the worker queue")
        .init()
});

pub fn register_gauge_metrics() -> Result<()> {
    METER.register_callback(|ctx| {
        RUNNER_COUNT_GAUGE.observe(ctx, conf::CONFIG.thread_counts.runner as u64, &[])
    })?;

    Ok(())
}
