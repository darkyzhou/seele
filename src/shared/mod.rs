use std::{env, io::SeekFrom, sync::LazyLock};

use anyhow::Result;
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncSeekExt, BufReader},
};

pub mod cond;
pub mod file;
pub mod http;
pub mod image;
pub mod metrics;
pub mod runner;

pub static TINI_PRESENTS: LazyLock<bool> = LazyLock::new(|| env::var_os("TINI_VERSION").is_some());

pub static ABORTED_MESSAGE: &str = "Aborted due to shutting down";

pub async fn tail(file: File, count: u64) -> Result<Vec<u8>> {
    let metadata = file.metadata().await?;
    let mut reader = BufReader::new(file);
    if metadata.len() > count {
        reader.seek(SeekFrom::End((count as i64).wrapping_neg())).await?;
    }

    let mut buffer = vec![];
    reader.read_to_end(&mut buffer).await?;
    Ok(buffer)
}
