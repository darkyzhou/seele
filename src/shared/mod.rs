use std::{env, io::SeekFrom};

use anyhow::Result;
use once_cell::sync::Lazy;
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncSeekExt, BufReader},
};

pub mod cond;
pub mod file;
pub mod image;
pub mod metrics;
pub mod runner;

pub static TINI_PRESENTS: Lazy<bool> = Lazy::new(|| env::var_os("TINI_VERSION").is_some());

pub static ABORTED_MESSAGE: &str = "Aborted due to shutting down";

#[inline]
pub fn random_task_id() -> String {
    nano_id::base62::<8>()
}

pub async fn tail(file: File, count: i64) -> Result<Vec<u8>> {
    let mut reader = BufReader::new(file);
    reader.seek(SeekFrom::End(-count)).await?;

    let mut buffer = vec![];
    reader.read_to_end(&mut buffer).await?;
    Ok(buffer)
}
