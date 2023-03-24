use std::{collections::HashMap, path::Path};

use anyhow::{bail, Context, Result};
use futures_util::future;
use once_cell::sync::Lazy;
use reqwest::{
    multipart::{Form, Part},
    Client,
};
use tokio::{
    fs::{self, File},
    io::{AsyncReadExt, BufReader},
};
use tracing::{info, instrument};

use crate::{
    entities::{
        SubmissionReportEmbedConfig, SubmissionReportUploadConfig, SubmissionReportUploadMethod,
    },
    shared,
};

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

                    info!(path = %path.display(), "Ignored a missing file to embed");
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

static HTTP_CLIENT: Lazy<Client> = Lazy::new(|| shared::http::build_http_client());

#[instrument(skip(root))]
pub async fn apply_uploads_config(
    root: &Path,
    uploads: &[SubmissionReportUploadConfig],
) -> Result<()> {
    let results = future::join_all(uploads.iter().map(|config| async move {
        let path = root.join(&config.path);

        async {
            let Ok(metadata) = fs::metadata(&path).await else {
                if !config.ignore_if_missing {
                    bail!("Failed to open the file");
                }

                info!(path = %path.display(), "Ignored a missing file to upload");
                return Ok(());
            };

            let file = File::open(&path).await.context("Failed to open the file")?;
            let builder = match config.method {
                SubmissionReportUploadMethod::Post => HTTP_CLIENT.post(config.target.clone()),
                SubmissionReportUploadMethod::Put => HTTP_CLIENT.put(config.target.clone()),
            };
            let form = Form::new()
                .part(config.form_field.clone(), Part::stream_with_length(file, metadata.len()));
            let response =
                builder.multipart(form).send().await.context("Error sending the request")?;

            if !response.status().is_success() {
                let text = response.text().await.context("Error reading the failed response")?;
                bail!("Remote server returned a failed response: {text}");
            }

            Ok(())
        }
        .await
        .with_context(|| format!("Error uploading the file: {}", path.display()))
    }))
    .await;

    let error = results
        .into_iter()
        .filter_map(|item| item.err())
        .map(|err| format!("{err:#}"))
        .collect::<Vec<_>>()
        .join("\n");
    if !error.is_empty() {
        bail!("Failed to upload some files:\n{error}");
    }

    Ok(())
}
