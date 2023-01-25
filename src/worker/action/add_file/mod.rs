use super::ActionContext;
use crate::{
    conf,
    entity::ActionExecutionReport,
    shared::{self, cond_group::CondGroup},
};
use anyhow::{anyhow, bail, Context};
use bytes::Bytes;
use futures_util::FutureExt;
use once_cell::sync::Lazy;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::io;
use tracing::instrument;

pub use self::config::*;

mod config;

static HTTP_CLIENT: Lazy<reqwest_middleware::ClientWithMiddleware> = Lazy::new(|| {
    use http_cache::MokaManager;
    use http_cache_reqwest::{Cache, CacheMode, HttpCache};
    use reqwest::Client;
    use reqwest_middleware::ClientBuilder;
    use std::time::Duration;

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
        // TODO: If the revalidation request fails (for example, on a 500 or if you’re offline), the stale response will be returned.
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
pub async fn add_file(
    ctx: &ActionContext,
    config: &ActionAddFileConfig,
) -> anyhow::Result<ActionExecutionReport> {
    let results = futures_util::future::join_all(config.files.iter().map(|item| async move {
        match item {
            ActionAddFileFileItem::Inline { path, text } => {
                handle_inline_file(ctx, path, text).await
            }
            ActionAddFileFileItem::Http { path, url } => handle_http_file(ctx, path, url).await,
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
        bail!("Failed to handle some of the files:\n{}", failed_items.join("\n"));
    }

    Ok(ActionExecutionReport::AddFile)
}

async fn handle_inline_file(ctx: &ActionContext, path: &Path, text: &str) -> anyhow::Result<()> {
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
async fn handle_http_file(ctx: &ActionContext, path: &Path, url: &String) -> anyhow::Result<()> {
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
        .map_err(|err| format!("Error sending the request: {:#}", err))?
        .bytes()
        .await
        .map_err(|err| format!("Error downloading the content: {:#}", err))
}

#[cfg(test)]
mod tests {
    use crate::worker::action::ActionContext;
    use std::{iter, path::PathBuf, sync::Arc};
    use tokio::fs;

    #[tokio::test]
    async fn test_handle_inline_file() {
        let submission_root: PathBuf = iter::once("./test_inline").collect();
        let target_path: PathBuf = iter::once("foo/bar.txt").collect();
        fs::create_dir_all(&submission_root).await.unwrap();

        let text = "EXAMPLE 测试".to_string();
        super::handle_inline_file(
            &ActionContext {
                submission_root: submission_root.clone(),
                submission_eviction_manager: Arc::default(),
                image_eviction_manager: Arc::default(),
            },
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
            &ActionContext {
                submission_root: submission_root.clone(),
                submission_eviction_manager: Arc::default(),
                image_eviction_manager: Arc::default(),
            },
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