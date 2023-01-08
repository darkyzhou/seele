use std::{iter, process::Output};

pub mod cond_group;
pub mod file_utils;
pub mod oci_image;

#[inline]
pub fn random_task_id() -> String {
    nano_id::base62::<8>()
}

#[inline]
pub fn collect_output(output: &Output) -> String {
    const MAX_LEN: usize = 400;

    let output = output
        .stdout
        .iter()
        .chain(iter::once(&b'\n'))
        .chain(output.stderr.iter())
        .take(MAX_LEN)
        .copied()
        .collect::<Vec<_>>();
    String::from_utf8_lossy(&output[..]).into_owned()
}
