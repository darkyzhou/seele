use std::{
    fs::File,
    io::{BufRead, BufReader},
};

use anyhow::{Context, Result, bail};
use once_cell::sync::Lazy;

use crate::conf;

const SUBUID_PATH: &str = "/etc/subuid";
const SUBGID_PATH: &str = "/etc/subgid";

pub struct SubIds {
    pub begin: u32,
    pub count: u32,
}

pub static SUBUIDS: Lazy<SubIds> =
    Lazy::new(|| get_subids(SUBUID_PATH).expect("Error getting subuids"));

pub static SUBGIDS: Lazy<SubIds> =
    Lazy::new(|| get_subids(SUBGID_PATH).expect("Error getting subgids"));

fn get_subids(path: &str) -> Result<SubIds> {
    let username = &conf::CONFIG.worker.action.run_container.userns_user;
    let reader = BufReader::new(File::open(path)?);
    get_subids_impl(username, reader.lines().flatten())
        .with_context(|| format!("Error getting subids from {path}"))
}

fn get_subids_impl(username: &str, lines: impl Iterator<Item = String>) -> Result<SubIds> {
    for line in lines {
        match line.split(':').collect::<Vec<_>>()[..] {
            [name, _, _] if name != username => continue,
            [_, begin, count] => {
                return Ok(SubIds { begin: begin.parse()?, count: count.parse()? });
            }
            _ => bail!("Unexpected line: {line}"),
        }
    }

    bail!("Cannot find the entry for username {username}");
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_get_subids() {
        let bad_cases = vec![
            ("seele", ""),
            ("seele", "seele:"),
            ("seele", "bronya:114514:233"),
            ("seele", "bronya:114514:233\nvollerei:123123:123"),
        ];

        let good_cases = vec![
            ("seele", "seele:100000:65536", (100000, 65536)),
            ("seele", "yzy1:100000:65536\nseele:165536:65536", (165536, 65536)),
            (
                "bronya",
                "yzy1:100000:65536\nseele:165536:65536\nbronya:1145141919:233",
                (1145141919, 233),
            ),
            (
                "seele",
                "yzy1:100000:65536\nseele:165536:65536\nbronya:1145141919:233\nseele:233:123",
                (165536, 65536),
            ),
        ];

        for (username, content) in bad_cases {
            let result =
                super::get_subids_impl(username, content.split('\n').map(|item| item.to_owned()));
            assert!(result.is_err());
        }

        for (username, content, (begin, count)) in good_cases {
            let result =
                super::get_subids_impl(username, content.split('\n').map(|item| item.to_owned()));
            assert!(result.is_ok());

            let ids = result.unwrap();
            assert_eq!((ids.begin, ids.count), (begin, count));
        }
    }
}
