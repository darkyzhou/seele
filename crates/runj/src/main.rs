use std::io::{self, Read};

use anyhow::{Context, Result};
use seele_shared::entities::run_container::runj as ent;

fn main() -> Result<()> {
    let mut input = String::new();
    if let Ok(file) = std::env::var("RUNJ_FILE") {
        input = std::fs::read_to_string(&file).with_context(|| format!("read {file}"))?;
    } else {
        io::stdin().read_to_string(&mut input).context("read stdin")?;
    }

    let config: ent::RunjConfig = serde_json::from_str(&input).context("parse config JSON")?;
    let report = runj::execute(&config).context("execute runj")?;
    let json = serde_json::to_string(&report).context("serialize report")?;
    println!("{}", json);
    Ok(())
}
