use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct TelemetryConfig {
    #[serde(default = "default_histogram_boundaries")]
    pub histogram_boundaries: Vec<f64>,

    pub collector_url: String,
}

#[inline]
fn default_histogram_boundaries() -> Vec<f64> {
    vec![0.05, 0.3, 1.8, 4.0, 8.0, 12.0, 16.0, 30.0, 60.0]
}
