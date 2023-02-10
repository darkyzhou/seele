use once_cell::sync::{Lazy, OnceCell};
use opentelemetry::{
    global,
    metrics::{Histogram, Meter, Unit},
    sdk::metrics::controllers::BasicController,
};

pub static METRICS_CONTROLLER: OnceCell<BasicController> = OnceCell::new();

pub static METER: Lazy<Meter> = Lazy::new(|| global::meter(env!("CARGO_PKG_NAME")));

pub static SUBMISSION_HANDLING_HISTOGRAM: Lazy<Histogram<f64>> = Lazy::new(|| {
    METER
        .f64_histogram("submission.duration")
        .with_description("Duration of submissions handling")
        .with_unit(Unit::new("s"))
        .init()
});
