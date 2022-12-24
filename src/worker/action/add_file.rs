use super::ActionContext;
use crate::{
    conf,
    entity::{ActionAddFileConfig, ActionAddFileFileItem, TaskSuccessReportExtra},
    shared,
};
use anyhow::Context;
use futures_util::TryStreamExt;
use once_cell::sync::Lazy;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tokio_util::io::StreamReader;

static HTTP_CLIENT: Lazy<reqwest_middleware::ClientWithMiddleware> = Lazy::new(|| {
    use http_cache_reqwest::{CACacheManager, Cache, CacheMode, HttpCache};
    use reqwest::Client;
    use reqwest_middleware::ClientBuilder;

    let cache_path = [&conf::CONFIG.root_path, "http_cache"]
        .iter()
        .collect::<PathBuf>()
        .into_os_string()
        .into_string()
        .unwrap();

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

pub async fn run_add_file_action(
    ctx: &ActionContext<'_>,
    config: &ActionAddFileConfig,
) -> anyhow::Result<TaskSuccessReportExtra> {
    futures_util::future::join_all(config.files.iter().map(|item| async move {
        match item {
            ActionAddFileFileItem::Inline { path, text } => handle_inline_file(ctx, path, text)
                .await
                .with_context(|| format!("Error handling the inline file item {}", path.display())),
            ActionAddFileFileItem::Http { path, url } => handle_http_file(ctx, path, url)
                .await
                .with_context(|| format!("Error handling the http file item {}", path.display())),
        }
    }))
    .await;

    Ok(TaskSuccessReportExtra::AddFile)
}

async fn handle_inline_file(
    ctx: &ActionContext<'_>,
    path: &Path,
    text: &str,
) -> anyhow::Result<()> {
    let mut file = {
        let path: PathBuf = [ctx.submission_root, path].iter().collect();
        shared::file_utils::create_file(&path).await?
    };
    let mut text = text.as_bytes();
    tokio::io::copy_buf(&mut text, &mut file).await?;
    Ok(())
}

async fn handle_http_file(ctx: &ActionContext<'_>, path: &Path, url: &str) -> anyhow::Result<()> {
    let mut file = {
        let path: PathBuf = [ctx.submission_root, path].iter().collect();
        shared::file_utils::create_file(&path).await?
    };

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

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::worker::action::ActionContext;
    use std::path::{Path, PathBuf};
    use tokio::fs;

    #[tokio::test]
    async fn test_handle_inline_file() {
        let submission_root = Path::new("./test_inline");
        let target_path = Path::new("foo/bar.txt");
        fs::create_dir_all(submission_root).await.unwrap();

        let text = "EXAMPLE 测试".to_string();
        super::handle_inline_file(&ActionContext { submission_root }, target_path, &text)
            .await
            .unwrap();

        assert_eq!(
            text,
            fs::read_to_string([submission_root, target_path].iter().collect::<PathBuf>())
                .await
                .unwrap()
        );

        fs::remove_dir_all(submission_root).await.unwrap();
    }

    #[tokio::test]
    async fn test_handle_http_file() {
        let submission_root = Path::new("./test_http");
        let target_path = Path::new("foo/bar.json");
        fs::create_dir_all(submission_root).await.unwrap();

        super::handle_http_file(
            &ActionContext { submission_root },
            target_path,
            "https://reqbin.com/echo/get/json",
        )
        .await
        .unwrap();

        assert_eq!(
            "{\"success\":\"true\"}\n",
            fs::read_to_string([submission_root, target_path].iter().collect::<PathBuf>())
                .await
                .unwrap()
        );

        fs::remove_dir_all(submission_root).await.unwrap();
    }
}
