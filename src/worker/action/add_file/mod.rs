use std::{path::PathBuf, sync::Arc};

use anyhow::{bail, Context, Result};
use bytes::Bytes;
use futures_util::{future, FutureExt};
use once_cell::sync::Lazy;
use tokio::{fs::File, io};
use tracing::instrument;
use triggered::Listener;

pub use self::entities::*;
use super::ActionContext;
use crate::{
    conf,
    entities::{ActionFailureReportExt, ActionReportExt, ActionSuccessReportExt},
    shared::{self, cond::CondGroup},
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
    use http_cache_reqwest::{Cache, CacheMode, HttpCache};
    use reqwest_middleware::ClientBuilder;

    ClientBuilder::new(shared::http::build_http_client())
        .with(Cache(HttpCache {
            // TODO: If the revalidation request fails (for example, on a 500 or if you’re offline),
            // the stale response will be returned.
            mode: CacheMode::Default,
            manager: MokaManager::new({
                use moka::future::Cache;

                let config = &conf::CONFIG.worker.action.add_file;
                Cache::builder()
                    .name("seele-add-file")
                    .weigher(|_, value: &Arc<Vec<u8>>| -> u32 {
                        value.len().try_into().unwrap_or(u32::MAX)
                    })
                    .max_capacity(1024 * 1024 * config.cache_size_mib)
                    .time_to_idle(Duration::from_secs(60 * 60 * config.cache_ttl_hour))
                    .build()
            }),
            options: None,
        }))
        .build()
});

static HTTP_TASKS: Lazy<CondGroup<String, Result<Bytes, String>>> =
    Lazy::new(|| CondGroup::new(|url: &String| download_http_file(url.clone()).boxed()));

#[instrument(skip(handle, file))]
async fn handle_http_file(handle: Listener, mut file: File, url: &String) -> Result<()> {
    match HTTP_TASKS.run(url.clone(), handle).await {
        None => bail!(shared::ABORTED_MESSAGE),
        Some(Err(err)) => bail!("Error downloading the file: {err:#}"),
        Some(Ok(data)) => {
            let mut data = data.as_ref();
            io::copy_buf(&mut data, &mut file).await.context("Error writing the data")?;
            Ok(())
        }
    }
}

async fn download_http_file(url: String) -> Result<Bytes, String> {
    // TODO: We should use streams, but need to find a way
    // to share it across CondGroup consumers
    HTTP_CLIENT
        .get(url)
        .send()
        .await
        .map_err(|err| format!("Error sending the request: {err:#}"))?
        .error_for_status()
        .map_err(|err| format!("Got a non-ok response: {err:#}"))?
        .bytes()
        .await
        .map_err(|err| format!("Error downloading the content: {err:#}"))
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
