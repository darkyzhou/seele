use std::io::SeekFrom;

use anyhow::Result;
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncSeekExt, BufReader},
};

pub mod cond_group;
pub mod file;
pub mod image;

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
