use std::path::PathBuf;

use anyhow::Result;
use chess_pos_analyzer::{analyze_fen, default_stockfish_path};
use clap::Parser;

const PYDANTIC_MODEL: &str = r#"from typing import List, Optional

from pydantic import BaseModel, Field


class PositionInfo(BaseModel):
    fen: str = Field(description="The analyzed chess position in FEN format.")
    hero: str = Field(description="The side to move in the FEN: 'w' for white, 'b' for black.")


class AnalysisConstants(BaseModel):
    depths_stability: List[int] = Field(description="Search depths used to estimate evaluation stability.")
    depth_main: int = Field(description="Main search depth used for root MultiPV and forcing counts.")
    pv_max_plies: int = Field(description="Maximum number of plies kept from each principal variation.")
    multipv_k: int = Field(description="Number of root principal variations requested from the engine.")


class MultipvLine(BaseModel):
    rank: int = Field(description="MultiPV rank at the root (1 is the best line).")
    move_uci: str = Field(description="First move of the line in UCI format.")
    score_pawns: Optional[float] = Field(
        description="Evaluation in full pawns from White's perspective (+ is better for White, - for Black), rounded to 2 decimals."
    )
    mate_in: Optional[int] = Field(
        description="Mate score from White's perspective (+ means White mates in N, - means Black mates in N)."
    )


class MultipvRoot(BaseModel):
    depth: int = Field(description="Search depth used for root MultiPV.")
    lines: List[MultipvLine] = Field(description="Root candidate lines sorted by MultiPV rank.")


class EvalByDepth(BaseModel):
    depth: int = Field(description="Search depth for this candidate evaluation.")
    score_pawns: Optional[float] = Field(
        description="Evaluation in full pawns from White's perspective (+ is better for White, - for Black), rounded to 2 decimals."
    )
    mate_in: Optional[int] = Field(
        description="Mate score from White's perspective (+ means White mates in N, - means Black mates in N)."
    )
    pv_uci: List[str] = Field(description="Principal variation moves in UCI format.")
    pv_san: List[str] = Field(description="Principal variation moves in SAN format.")


class Stability(BaseModel):
    has_mate: bool = Field(description="True if any depth reports a mate score.")
    range_pawns: Optional[float] = Field(
        description="Range of score_pawns across depths (max - min), rounded to 2 decimals."
    )
    mean_pawns: Optional[float] = Field(
        description="Mean of score_pawns across depths, rounded to 2 decimals."
    )
    std_pawns: Optional[float] = Field(
        description="Standard deviation of score_pawns across depths, rounded to 2 decimals."
    )
    pv_common_prefix_plies: int = Field(description="Length of the shared PV prefix across depths.")
    pv_stability_ratio: float = Field(
        description="Shared PV prefix length divided by main-depth PV length."
    )


class TacticalFlags(BaseModel):
    candidate_is_check: bool = Field(description="True if the candidate move gives check.")
    candidate_is_capture: bool = Field(description="True if the candidate move captures a piece.")
    candidate_is_promotion: bool = Field(description="True if the candidate move promotes a pawn.")


class ForcingCounts(BaseModel):
    checks: int = Field(description="Number of checking moves in the main PV (first up to 12 plies).")
    captures: int = Field(description="Number of capture moves in the main PV (first up to 12 plies).")
    promotions: int = Field(description="Number of promotions in the main PV (first up to 12 plies).")


class CandidateOutput(BaseModel):
    move_uci: str = Field(description="Candidate move in UCI format.")
    move_san: str = Field(description="Candidate move in SAN format.")
    eval_by_depth: List[EvalByDepth] = Field(description="Per-depth evaluation and PV details.")
    stability: Stability = Field(description="Stability metrics derived from eval_by_depth.")
    tactical_flags: TacticalFlags = Field(description="Basic tactical markers for the candidate move.")
    pv_forcing_counts_first_12plies: ForcingCounts = Field(
        description="Counts of forcing move types in the main PV, truncated to 12 plies."
    )


class AnalyzerOutput(BaseModel):
    schema_version: str = Field(description="Schema version of this analyzer output.")
    position: PositionInfo = Field(description="Input position metadata.")
    analysis_constants: AnalysisConstants = Field(description="Constants used by the analysis pipeline.")
    multipv_from_root: MultipvRoot = Field(description="Top root engine lines at depth_main.")
    candidates: List[CandidateOutput] = Field(description="Detailed analysis for every legal move.")
"#;

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
    println!("```python\n{PYDANTIC_MODEL}\n```");
    println!("```json\n{}\n```", serde_json::to_string_pretty(&out)?);
    Ok(())
}
