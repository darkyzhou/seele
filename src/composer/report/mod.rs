use std::{collections::HashMap, sync::Arc};

use anyhow::{bail, Context, Result};
use futures_util::future;
use tokio::{
    fs::{self, File},
    io::{AsyncReadExt, BufReader},
};

use crate::entities::{Submission, SubmissionConfig, SubmissionReportConfig, SubmissionReporter};

mod javascript;
mod utils;

pub async fn make_submission_report(
    config: Arc<SubmissionConfig>,
    reporter: &SubmissionReporter,
) -> Result<SubmissionReportConfig> {
    match reporter {
        SubmissionReporter::JavaScript { javascript } => {
            javascript::execute_javascript_reporter(
                serde_json::to_string(&config)?,
                javascript.to_string(),
            )
            .await
        }
    }
}

pub struct ApplyReportConfigResult {
    pub embeds: HashMap<String, String>,
}

pub async fn apply_report_config(
    config: &SubmissionReportConfig,
    submission: &Submission,
) -> Result<ApplyReportConfigResult> {
    if !config.uploads.is_empty() {
        bail!("`uploads` is not implemented");
    }

    let embeds = HashMap::from_iter({
        future::try_join_all(config.embeds.iter().map(|embed| async move {
            // TODO: Should we check for malicious paths?
            let path = submission.root_directory.join(&embed.path);

            async {
                let metadata = fs::metadata(&path).await.context("Error checking metadata")?;
                let truncate_bytes = embed.truncate_kib * 1024;
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

                anyhow::Ok((embed.field.clone(), content))
            }
            .await
            .with_context(|| format!("Error handling the file: {}", path.display()))
        }))
        .await
        .context("Error applying embeds config")?
    });

    Ok(ApplyReportConfigResult { embeds })
}
