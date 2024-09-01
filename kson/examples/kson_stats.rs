extern crate anyhow;
extern crate clap;
extern crate kson;
extern crate serde_json;

use std::{io::BufReader, path::PathBuf};

use anyhow::Result;
use clap::Parser;

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Name of the person to greet
    #[clap(short, long, value_parser)]
    infile: PathBuf,
}
pub fn main() -> Result<()> {
    let Args { infile } = Args::parse();

    let reader = BufReader::new(std::fs::File::open(infile)?);

    let chart: kson::Chart = serde_json::from_reader(reader)?;

    let score_ticks = kson::score_ticks::generate_score_ticks(&chart);

    let tick_types = score_ticks
        .iter()
        .fold((0, 0, 0, 0), |acc, tick| match tick.tick {
            kson::score_ticks::ScoreTick::Chip { lane: _ } => (acc.0 + 1, acc.1, acc.2, acc.3),
            kson::score_ticks::ScoreTick::Hold { .. } => (acc.0, acc.1 + 1, acc.2, acc.3),
            kson::score_ticks::ScoreTick::Laser { lane: _, pos: _ } => {
                (acc.0, acc.1, acc.2 + 1, acc.3)
            }
            kson::score_ticks::ScoreTick::Slam {
                lane: _,
                start: _,
                end: _,
            } => (acc.0, acc.1, acc.2, acc.3 + 1),
        });

    println!("Chip:\t{}", tick_types.0);
    println!("Hold:\t{}", tick_types.1);
    println!("Laser:\t{}", tick_types.2);
    println!("Slam:\t{}", tick_types.3);
    println!("----");
    println!("Total:\t{}", score_ticks.len());

    Ok(())
}
