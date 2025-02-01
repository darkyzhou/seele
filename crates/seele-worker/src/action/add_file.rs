use std::{
    path::{Path, PathBuf},
    sync::{Arc, LazyLock},
};

use anyhow::{Context, Result, bail};
use bytes::Bytes;
use futures_util::{Stream, StreamExt, future};
use http_cache::HttpCacheOptions;
use seele_shared::entities::add_file::*;
use tokio::{
    fs::File,
    io::{self, AsyncWriteExt},
    task::spawn_blocking,
};
use tracing::{info, instrument};
use triggered::Listener;

use super::ActionContext;
use crate::{
    conf,
    entities::{ActionFailureReportExt, ActionReportExt, ActionSuccessReportExt},
    shared,
};

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
                FileItemExt::PlainText { plain } => handle_plain_text(file, plain).await,
                FileItemExt::Http { url } => handle_http_url(handle, file, url).await,
                FileItemExt::Base64 { base64 } => handle_base64(file, base64).await,
                FileItemExt::LocalPath { local } => handle_local_path(file, local).await,
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

async fn handle_plain_text(mut file: File, text: &str) -> Result<()> {
    let mut text = text.as_bytes();
    io::copy_buf(&mut text, &mut file).await.context("Error writing the file")?;
    Ok(())
}

async fn handle_base64(mut file: File, base64: &str) -> Result<()> {
    use base64::prelude::*;

    let data = spawn_blocking({
        let base64 = base64.to_owned();
        move || BASE64_STANDARD_NO_PAD.decode(base64)
    })
    .await?
    .context("Error decoding base64 text")?;
    io::copy_buf(&mut data.as_slice(), &mut file).await.context("Error writing the file")?;
    Ok(())
}

async fn handle_local_path(mut file: File, path: &Path) -> Result<()> {
    let mut source = File::open(path).await.context("Error opening the file")?;
    io::copy(&mut source, &mut file).await.context("Error copying the file")?;
    Ok(())
}

static HTTP_CLIENT: LazyLock<reqwest_middleware::ClientWithMiddleware> = LazyLock::new(|| {
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
            options: HttpCacheOptions::default(),
        }))
        .build()
});

#[instrument(skip(handle, file))]
async fn handle_http_url(handle: Listener, mut file: File, url: &str) -> Result<()> {
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
            info!("Cache served: {:?}, cache existed: {:?}", cache, cache_lookup);
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
    use std::path::Path;

    use tokio::fs::{self, File};

    #[tokio::test]
    async fn test_handle_inline() {
        const PATH: &str = "./test-inline.txt";

        let file = File::create(PATH).await.unwrap();
        let text = "EXAMPLE 测试".to_string();

        super::handle_plain_text(file, &text).await.unwrap();

        assert_eq!(fs::read_to_string(PATH).await.unwrap(), text);

        fs::remove_file(PATH).await.unwrap();
    }

    #[tokio::test]
    async fn test_handle_base64() {
        const PATH: &str = "./test-base64.txt";

        let file = File::create(PATH).await.unwrap();
        let base64 = "5biM5YS/5pyA5Y+v54ix5LqG".to_string();

        super::handle_base64(file, &base64).await.unwrap();

        assert_eq!(fs::read_to_string(PATH).await.unwrap(), "希儿最可爱了");

        fs::remove_file(PATH).await.unwrap();
    }

    #[tokio::test]
    async fn test_handle_http_url() {
        const PATH: &str = "./test-url.txt";

        let file = File::create(PATH).await.unwrap();
        let (_trigger, listener) = triggered::trigger();
        super::handle_http_url(listener, file, "https://httpbin.io/user-agent").await.unwrap();

        let ua = &super::conf::CONFIG.http.user_agent;
        assert_eq!(
            fs::read_to_string(PATH).await.unwrap(),
            format!("{{\n  \"user-agent\": \"{}\"\n}}\n", ua)
        );

        fs::remove_file(PATH).await.unwrap();
    }

    #[tokio::test]
    async fn test_handle_local_path() {
        const SOURCE_PATH: &str = "./test-local-source.txt";
        const TARGET_PATH: &str = "./test-local-target.txt";
        const TEXT: &str = "希儿最可爱了test114514";

        fs::write(SOURCE_PATH, TEXT).await.unwrap();

        let file = File::create(TARGET_PATH).await.unwrap();
        super::handle_local_path(file, Path::new(SOURCE_PATH)).await.unwrap();

        assert_eq!(fs::read_to_string(TARGET_PATH).await.unwrap(), TEXT);

        fs::remove_file(TARGET_PATH).await.unwrap();
        fs::remove_file(SOURCE_PATH).await.unwrap();
    }
}
