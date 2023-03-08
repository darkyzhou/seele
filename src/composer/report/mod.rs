use std::{collections::HashMap, path::Path};

use anyhow::{bail, Context, Result};
use futures_util::future;
use serde_json::Value;
use tokio::{
    fs::{self, File},
    io::{AsyncReadExt, BufReader},
};
use tracing::instrument;

use crate::entities::{Submission, SubmissionReportEmbedConfig, SubmissionReporter};

mod javascript;
mod utils;

#[instrument(skip_all)]
pub async fn execute_reporter(
    submission: &Submission,
    reporter: &SubmissionReporter,
    data: Value,
) -> Result<Value> {
    let mut config = match reporter {
        SubmissionReporter::JavaScript { javascript } => {
            javascript::execute_javascript_reporter(data, javascript.to_string()).await?
        }
    };

    let embeds = apply_embeds_config(&submission.root_directory, &config.embeds)
        .await
        .context("Error applying the embeds config")?;
    for (field, content) in embeds {
        config.report.insert(field, content.into());
    }

    serde_json::to_value(config.report).context("Error serializing the report from the reporter")
}

#[instrument(skip(root))]
pub async fn apply_embeds_config(
    root: &Path,
    embeds: &[SubmissionReportEmbedConfig],
) -> Result<HashMap<String, String>> {
    Ok(HashMap::from_iter({
        future::try_join_all(embeds.iter().map(|config| async move {
            // TODO: Should we check for malicious paths?
            let path = root.join(&config.path);

            async {
                let Ok(metadata) = fs::metadata(&path).await else {
                    if !config.ignore_if_missing {
                        bail!("Failed to open the file");
                    }

                    return Ok(None);
                };

                let truncate_bytes = config.truncate_kib * 1024;
                let content = {
                    let mut reader =
                        BufReader::new(File::open(&path).await.context("Error opening the file")?);
                    let mut buffer = Vec::with_capacity(truncate_bytes);
                    if metadata.len() as usize <= truncate_bytes {
                        reader.read_to_end(&mut buffer).await?;
                    } else {
                        reader.read_exact(&mut buffer).await?;
                    }
                    String::from_utf8_lossy(&buffer).to_string()
                };

                anyhow::Ok(Some((config.field.clone(), content)))
            }
            .await
            .with_context(|| format!("Error handling the file: {}", path.display()))
        }))
        .await?
        .into_iter()
        .filter_map(|item| item)
    }))
}
