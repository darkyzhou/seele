use either::Either;
use std::{iter, process::Output};

pub mod cond_group;
pub mod file_utils;
pub mod oci_image;

#[inline]
pub fn random_task_id() -> String {
    nano_id::base62::<8>()
}

macro_rules! skip_if_empty {
    ($source:expr, $iter:expr) => {
        if $source.is_empty() {
            Either::Left(iter::empty())
        } else {
            Either::Right($iter)
        }
    };
}

macro_rules! ellipse {
    ($source:expr, $max_len:expr) => {
        if $source.len() <= $max_len {
            Either::Left($source.iter())
        } else {
            Either::Right(
                $source
                    .iter()
                    .take($max_len / 2)
                    .chain(b"...".iter())
                    .chain($source.iter().skip($max_len / 2)),
            )
        }
    };
}

pub fn collect_output(output: &Output) -> String {
    const MAX_LEN: usize = 400;

    let output = skip_if_empty!(
        output.stdout,
        b"\n--- stdout ---\n".iter().chain(ellipse!(output.stdout, MAX_LEN))
    )
    .chain(skip_if_empty!(
        output.stderr,
        b"\n--- stderr ---\n".iter().chain(ellipse!(output.stderr, MAX_LEN))
    ))
    .copied()
    .collect::<Vec<_>>();
    String::from_utf8_lossy(&output[..]).into_owned()
}
