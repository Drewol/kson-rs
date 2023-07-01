extern crate anyhow;
extern crate clap;
extern crate kson;
extern crate serde_json;

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use kson::Ksh;

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Name of the person to greet
    #[clap(short, long, value_parser)]
    infile: PathBuf,
    #[clap(short, long, value_parser)]
    outfile: PathBuf,
    #[clap(long)]
    pretty: bool,
}

pub fn main() -> Result<()> {
    let Args {
        infile,
        outfile,
        pretty,
    } = Args::parse();
    let chart = kson::Chart::from_ksh(&std::fs::read_to_string(infile)?)?;
    let outfile = std::fs::File::create(outfile)?;
    if pretty {
        serde_json::to_writer_pretty(outfile, &chart)?;
    } else {
        serde_json::to_writer(outfile, &chart)?;
    }
    Ok(())
}
