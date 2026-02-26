use std::path::PathBuf;

use anyhow::Result;
use chess_pos_analyzer::{analyze_fen, default_stockfish_path};
use clap::Parser;

#[derive(Parser, Debug)]
#[command(version, about = "Analyze all legal moves for a FEN using Stockfish")]
struct Cli {
    #[arg(long)]
    fen: String,
    #[arg(long, default_value_os_t = default_stockfish_path())]
    stockfish: PathBuf,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let out = analyze_fen(&cli.fen, &cli.stockfish)?;
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}
