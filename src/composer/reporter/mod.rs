use std::path::Path;

use anyhow::{Context, Result};
use serde_json::Value;
use tracing::instrument;

use super::report::apply_embeds_config;
use crate::entities::{SubmissionReportUploadConfig, SubmissionReporter};

mod javascript;
mod utils;

#[instrument(skip_all)]
pub async fn execute_reporter(
    root_directory: &Path,
    reporter: &SubmissionReporter,
    data: Value,
) -> Result<(Value, Vec<SubmissionReportUploadConfig>)> {
    let mut config = match reporter {
        SubmissionReporter::JavaScript { javascript } => {
            javascript::execute_javascript_reporter(data, javascript.to_string()).await?
        }
    };

    let embeds = apply_embeds_config(root_directory, &config.embeds)
        .await
        .context("Error applying the embeds config")?;
    for (field, content) in embeds {
        config.report.insert(field, content.into());
    }

    let report = serde_json::to_value(config.report)
        .context("Error serializing the report from the reporter")?;
    Ok((report, config.uploads))
}
