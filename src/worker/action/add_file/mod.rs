use super::ActionContext;
use crate::{
    conf,
    entity::ActionExecutionReport,
    shared::{self, cond_group::CondGroup},
};
use anyhow::{anyhow, bail, Context};
use futures_util::{FutureExt, TryStreamExt};
use once_cell::sync::Lazy;
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    time::Duration,
};
use tokio_util::io::StreamReader;
use tracing::instrument;

pub use self::config::*;

mod config;

static HTTP_CLIENT: Lazy<reqwest_middleware::ClientWithMiddleware> = Lazy::new(|| {
    use http_cache_reqwest::{CACacheManager, Cache, CacheMode, HttpCache};
    use reqwest::Client;
    use reqwest_middleware::ClientBuilder;

    let cache_path = conf::PATHS.http_cache.to_str().unwrap().to_string();

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
        manager: CACacheManager { path: cache_path },
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
        shared::file_utils::create_file(&path).await?
    };
    let mut text = text.as_bytes();
    tokio::io::copy_buf(&mut text, &mut file).await?;
    Ok(())
}

static HTTP_TASKS: Lazy<CondGroup<String, Result<PathBuf, String>>> =
    Lazy::new(|| CondGroup::new(|url: &String| download_http_file(url.clone()).boxed()));

#[instrument]
async fn handle_http_file(ctx: &ActionContext, path: &Path, url: &String) -> anyhow::Result<()> {
    use tokio::fs;

    let src_path = HTTP_TASKS.run(url).await.map_err(|msg| anyhow!(msg))?;
    let target_path: PathBuf = ctx.submission_root.join(path);

    if let Some(parent_path) = target_path.parent() {
        fs::create_dir_all(parent_path).await?;
    }

    fs::hard_link(src_path, target_path).await?;

    Ok(())
}

async fn download_http_file(url: String) -> Result<PathBuf, String> {
    async {
        let file_path: PathBuf = {
            let mut hasher = DefaultHasher::new();
            url.hash(&mut hasher);
            conf::PATHS.downloads.join(format!("{:x}", hasher.finish()))
        };

        let mut file = shared::file_utils::create_file(&file_path).await?;

        let mut stream = {
            use std::io::{Error, ErrorKind};
            StreamReader::new(
                HTTP_CLIENT
                    .get(url)
                    .send()
                    .await
                    .context("Error sending the request")?
                    .bytes_stream()
                    .map_err(|err| Error::new(ErrorKind::Other, err)),
            )
        };

        tokio::io::copy(&mut stream, &mut file)
            .await
            .context("Error copying data from the response")?;

        anyhow::Result::<PathBuf>::Ok(file_path)
    }
    .await
    .map_err(|err| format!("Error downloading the http file: {:#}", err))
}

#[cfg(test)]
mod tests {
    use crate::worker::action::ActionContext;
    use std::{iter, path::PathBuf};
    use tokio::fs;

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
        let submission_root: PathBuf = iter::once("./test_inline").collect();
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
