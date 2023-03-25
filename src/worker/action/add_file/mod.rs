use std::{path::PathBuf, sync::Arc};

use anyhow::{bail, Context, Result};
use bytes::Bytes;
use futures_util::{future, Stream, StreamExt};
use once_cell::sync::Lazy;
use tokio::{
    fs::File,
    io::{self, AsyncWriteExt},
};
use tracing::{info, instrument};
use triggered::Listener;

pub use self::entities::*;
use super::ActionContext;
use crate::{
    conf,
    entities::{ActionFailureReportExt, ActionReportExt, ActionSuccessReportExt},
    shared,
};

mod entities;

#[instrument(skip_all, name = "action_add_file_execute")]
pub async fn execute(
    handle: Listener,
    ctx: &ActionContext,
    config: &Config,
) -> Result<ActionReportExt> {
    let results = future::join_all(config.files.iter().map(|item| {
        let handle = handle.clone();
        async move {
            let file = {
                let path: PathBuf = ctx.submission_root.join(&item.path);
                shared::file::create_file(&path).await.context("Error creating the file")?
            };

            match &item.ext {
                FileItemExt::PlainText { plain } => handle_plain_text_file(file, &plain).await,
                FileItemExt::Http { url } => handle_http_file(handle, file, url).await,
            }
        }
    }))
    .await;

    let failed_items: Vec<_> = results
        .iter()
        .enumerate()
        .filter_map(|(i, result)| {
            result.as_ref().err().map(|err| format!("{}: {:#}", config.files[i], err))
        })
        .collect();

    if !failed_items.is_empty() {
        return Ok(ActionReportExt::Failure(ActionFailureReportExt::AddFile(FailedReport {
            files: failed_items,
        })));
    }

    Ok(ActionReportExt::Success(ActionSuccessReportExt::AddFile))
}

async fn handle_plain_text_file(mut file: File, text: &str) -> Result<()> {
    let mut text = text.as_bytes();
    io::copy_buf(&mut text, &mut file).await.context("Error writing the data")?;
    Ok(())
}

static HTTP_CLIENT: Lazy<reqwest_middleware::ClientWithMiddleware> = Lazy::new(|| {
    use std::time::Duration;

    use http_cache::MokaManager;
    use http_cache_reqwest::{Cache, HttpCache};
    use reqwest_middleware::ClientBuilder;

    let config = &conf::CONFIG.worker.action.add_file;
    ClientBuilder::new(shared::http::build_http_client())
        .with(Cache(HttpCache {
            mode: config.cache_strategy.into(),
            manager: MokaManager::new(
                moka::future::Cache::builder()
                    .name("seele-add-file")
                    .weigher(|_, value: &Arc<Vec<u8>>| -> u32 {
                        value.len().try_into().unwrap_or(u32::MAX)
                    })
                    .max_capacity(1024 * 1024 * config.cache_size_mib)
                    .time_to_idle(Duration::from_secs(60 * 60 * config.cache_ttl_hour))
                    .build(),
            ),
            options: None,
        }))
        .build()
});

#[instrument(skip(handle, file))]
async fn handle_http_file(handle: Listener, mut file: File, url: &str) -> Result<()> {
    tokio::select! {
        _ = handle => bail!(shared::ABORTED_MESSAGE),
        result = download_http_file(url) => match result {
            Err(err) => bail!("Error downloading the file: {err:#}"),
            Ok(mut stream) => {
                while let Some(data) = stream.next().await {
                    let data = data.context("Error reading the remote data")?;
                    file.write_all(&data).await.context("Error writing to the file")?;
                }
                Ok(())
            }
        }
    }
}

async fn download_http_file(
    url: &str,
) -> Result<impl Stream<Item = Result<Bytes, reqwest::Error>>> {
    let response = HTTP_CLIENT
        .get(url)
        .send()
        .await
        .context("Error sending the request")?
        .error_for_status()
        .context("Got a non-ok response")?;

    let headers = response.headers();
    match (headers.get(http_cache::XCACHE), headers.get(http_cache::XCACHELOOKUP)) {
        (Some(cache), Some(cache_lookup)) => {
            info!("Cache served: {:?}, cache existed {:?}", cache, cache_lookup);
        }
        (Some(cache), None) => {
            info!("Cache served: {:?}", cache);
        }
        (None, Some(cache_lookup)) => {
            info!("Cache existed {:?}", cache_lookup);
        }
        _ => {}
    }

    Ok(response.bytes_stream())
}

#[cfg(test)]
mod tests {
    use tokio::fs::{self, File};

    #[tokio::test]
    async fn test_handle_inline_file() {
        const PATH: &str = "./test-inline.txt";

        let file = File::open(PATH).await.unwrap();
        let text = "EXAMPLE 测试".to_string();

        super::handle_plain_text_file(file, &text).await.unwrap();

        assert_eq!(fs::read_to_string(PATH).await.unwrap(), text);

        fs::remove_file(PATH).await.unwrap();
    }

    #[tokio::test]
    async fn test_handle_http_file() {
        const PATH: &str = "./test-base64.txt";

        let file = File::open(PATH).await.unwrap();
        let (_trigger, listener) = triggered::trigger();
        super::handle_http_file(listener, file, &"https://reqbin.com/echo/get/json".to_string())
            .await
            .unwrap();

        assert_eq!(fs::read_to_string(PATH).await.unwrap(), "{\"success\":\"true\"}\n");

        fs::remove_file(PATH).await.unwrap();
    }
}
