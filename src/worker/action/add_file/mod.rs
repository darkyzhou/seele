use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{anyhow, bail, Context, Result};
use bytes::Bytes;
pub use entities::*;
use futures_util::FutureExt;
use once_cell::sync::Lazy;
use tokio::io;
use tracing::instrument;

use super::ActionContext;
use crate::{
    conf,
    entities::{ActionFailedReportExt, ActionSuccessReportExt},
    shared::{self, cond_group::CondGroup},
    worker::ActionErrorWithReport,
};

mod entities;

static HTTP_CLIENT: Lazy<reqwest_middleware::ClientWithMiddleware> = Lazy::new(|| {
    use std::time::Duration;

    use http_cache::MokaManager;
    use http_cache_reqwest::{Cache, CacheMode, HttpCache};
    use reqwest::Client;
    use reqwest_middleware::ClientBuilder;

    ClientBuilder::new(
        Client::builder()
            .user_agent(concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(5)) // TODO: move to conf
            .timeout(Duration::from_secs(30)) // TODO: move to conf
            // TODO: pool_idle_timeout?
            .build()
            .unwrap(),
    )
    .with(Cache(HttpCache {
        // TODO: If the revalidation request fails (for example, on a 500 or if you’re offline), the
        // stale response will be returned.
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

#[instrument]
pub async fn execute(ctx: &ActionContext, config: &Config) -> Result<ActionSuccessReportExt> {
    let results = futures_util::future::join_all(config.files.iter().map(|item| async move {
        match &item.ext {
            FileItemExt::Inline { content } => handle_inline_file(ctx, &item.path, content).await,
            FileItemExt::Http { url } => handle_http_file(ctx, &item.path, url).await,
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
        bail!(ActionErrorWithReport::new(ActionFailedReportExt::AddFile(FailedReport {
            files: failed_items
        })));
    }

    Ok(ActionSuccessReportExt::AddFile)
}

async fn handle_inline_file(ctx: &ActionContext, path: &Path, text: &str) -> Result<()> {
    let mut file = {
        let path: PathBuf = ctx.submission_root.join(path);
        shared::file_utils::create_file(&path).await.context("Error creating the file")?
    };

    let mut text = text.as_bytes();
    io::copy_buf(&mut text, &mut file).await.context("Error writing the data")?;

    Ok(())
}

static HTTP_TASKS: Lazy<CondGroup<String, Result<Bytes, String>>> =
    Lazy::new(|| CondGroup::new(|url: &String| download_http_file(url.clone()).boxed()));

#[instrument]
async fn handle_http_file(ctx: &ActionContext, path: &Path, url: &String) -> Result<()> {
    let mut file = {
        let target_path: PathBuf = ctx.submission_root.join(path);
        shared::file_utils::create_file(&target_path).await.context("Error creating the file")?
    };

    let data =
        HTTP_TASKS.run(url).await.map_err(|msg| anyhow!("Error downloading the file: {}", msg))?;
    let mut data = data.as_ref();
    io::copy_buf(&mut data, &mut file).await.context("Error writing the data")?;

    Ok(())
}

async fn download_http_file(url: String) -> Result<Bytes, String> {
    // TODO: We should use streams, but need to find a way
    // to share it across CondGroup consumers
    HTTP_CLIENT
        .get(url)
        .send()
        .await
        .map_err(|err| format!("Error sending the request: {err:#}"))?
        .bytes()
        .await
        .map_err(|err| format!("Error downloading the content: {err:#}"))
}

#[cfg(test)]
mod tests {
    use std::{iter, path::PathBuf};

    use tokio::fs;

    use crate::worker::action::ActionContext;

    #[tokio::test]
    async fn test_handle_inline_file() {
        let submission_root: PathBuf = iter::once("./test_inline").collect();
        let target_path: PathBuf = iter::once("foo/bar.txt").collect();
        fs::create_dir_all(&submission_root).await.unwrap();

        let text = "EXAMPLE 测试".to_string();
        super::handle_inline_file(
            &ActionContext { submission_root: submission_root.clone() },
            &target_path,
            &text,
        )
        .await
        .unwrap();

        assert_eq!(text, fs::read_to_string(submission_root.join(target_path)).await.unwrap());

        fs::remove_dir_all(submission_root).await.unwrap();
    }

    #[tokio::test]
    async fn test_handle_http_file() {
        let submission_root: PathBuf = iter::once("./test_http").collect();
        let target_path: PathBuf = iter::once("foo/bar.txt").collect();
        fs::create_dir_all(&submission_root).await.unwrap();

        super::handle_http_file(
            &ActionContext { submission_root: submission_root.clone() },
            &target_path,
            &"https://reqbin.com/echo/get/json".to_string(),
        )
        .await
        .unwrap();

        assert_eq!(
            "{\"success\":\"true\"}\n",
            fs::read_to_string(submission_root.join(target_path)).await.unwrap()
        );

        fs::remove_dir_all(submission_root).await.unwrap();
    }
}
