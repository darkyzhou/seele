use std::{
    fs::File,
    io::{BufRead, BufReader},
};

use anyhow::{bail, Result};
use once_cell::sync::Lazy;

use crate::conf;

const SUBUID_PATH: &str = "/etc/subuid";
const SUBGID_PATH: &str = "/etc/subgid";

pub static SUBUIDS: Lazy<SubIds> = Lazy::new(|| {
    get_subuids(&conf::CONFIG.worker.action.run_container.userns_user)
        .expect("Error getting subuids")
});

pub static SUBGIDS: Lazy<SubIds> = Lazy::new(|| {
    get_subgids(&conf::CONFIG.worker.action.run_container.userns_group)
        .expect("Error getting subgids")
});

pub struct SubIds {
    pub begin: u32,
    pub count: u32,
}

fn get_subuids(name: &str) -> Result<SubIds> {
    get_subids(name, SUBUID_PATH)
}

fn get_subgids(name: &str) -> Result<SubIds> {
    get_subids(name, SUBGID_PATH)
}

fn get_subids(name: &str, path: &str) -> Result<SubIds> {
    let reader = BufReader::new(File::open(path)?);

    for line in reader.lines().flatten() {
        match line.split(':').collect::<Vec<_>>()[..] {
            [the_name, begin, count] => {
                if the_name != name {
                    continue;
                }

                return Ok(SubIds { begin: begin.parse()?, count: count.parse()? });
            }
            _ => bail!("Unexpected line: {}", line),
        }
    }

    bail!("Cannot find name {name} in {path}");
}
