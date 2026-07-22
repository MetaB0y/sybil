//! Paired FBA-versus-CLOB research runner.

use clap::Parser;

#[path = "../market_structure.rs"]
mod market_structure;
#[path = "../witness.rs"]
mod witness;

fn main() {
    if let Err(error) = market_structure::run(market_structure::Cli::parse()) {
        eprintln!("market-structure experiment failed: {error}");
        std::process::exit(1);
    }
}
