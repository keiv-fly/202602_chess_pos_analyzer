use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use anyhow::{Context, Result, bail};
use indicatif::{ProgressBar, ProgressStyle};
use serde::Serialize;
use shakmaty::fen::Fen;
use shakmaty::san::San;
use shakmaty::uci::UciMove;
use shakmaty::{CastlingMode, Chess, Color, Move, Position};

pub const DEPTHS_STABILITY: [u32; 3] = [12, 16, 20];
pub const DEPTH_MAIN: u32 = 20;
pub const PV_MAX_PLIES: usize = 12;
pub const MULTIPV_K: usize = 6;

#[derive(Debug, Serialize)]
pub struct AnalyzerOutput {
    pub schema_version: String,
    pub position: PositionInfo,
    pub analysis_constants: AnalysisConstants,
    pub multipv_from_root: MultipvRoot,
    pub candidates: Vec<CandidateOutput>,
}

#[derive(Debug, Serialize)]
pub struct PositionInfo {
    pub fen: String,
    pub hero: String,
}

#[derive(Debug, Serialize)]
pub struct AnalysisConstants {
    pub depths_stability: [u32; 3],
    pub depth_main: u32,
    pub pv_max_plies: usize,
    pub multipv_k: usize,
}

#[derive(Debug, Serialize)]
pub struct MultipvRoot {
    pub depth: u32,
    pub lines: Vec<MultipvLine>,
}

#[derive(Debug, Serialize)]
pub struct MultipvLine {
    pub rank: usize,
    pub move_uci: String,
    pub score_pawns: Option<f64>,
    pub mate_in: Option<i32>,
}

#[derive(Debug, Serialize)]
pub struct CandidateOutput {
    pub move_uci: String,
    pub move_san: String,
    pub eval_by_depth: Vec<EvalByDepth>,
    pub stability: Stability,
    pub tactical_flags: TacticalFlags,
    pub pv_forcing_counts_first_12plies: ForcingCounts,
}

#[derive(Debug, Serialize)]
pub struct EvalByDepth {
    pub depth: u32,
    pub score_pawns: Option<f64>,
    pub mate_in: Option<i32>,
    pub pv_uci: Vec<String>,
    pub pv_san: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct Stability {
    pub has_mate: bool,
    pub range_pawns: Option<f64>,
    pub mean_pawns: Option<f64>,
    pub std_pawns: Option<f64>,
    pub pv_common_prefix_plies: usize,
    pub pv_stability_ratio: f64,
}

#[derive(Debug, Serialize)]
pub struct TacticalFlags {
    pub candidate_is_check: bool,
    pub candidate_is_capture: bool,
    pub candidate_is_promotion: bool,
}

#[derive(Debug, Serialize, Default)]
pub struct ForcingCounts {
    pub checks: usize,
    pub captures: usize,
    pub promotions: usize,
}

#[derive(Debug)]
struct ParsedInfo {
    depth: Option<u32>,
    multipv: usize,
    cp: Option<i32>,
    mate: Option<i32>,
    pv: Vec<String>,
}

pub fn default_stockfish_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        PathBuf::from("stockfish/stockfish.exe")
    }
    #[cfg(not(target_os = "windows"))]
    {
        PathBuf::from("stockfish/stockfish")
    }
}

pub fn analyze_fen(fen: &str, stockfish_path: &Path) -> Result<AnalyzerOutput> {
    let fen_obj: Fen = fen.parse().context("invalid FEN")?;
    let pos: Chess = fen_obj
        .into_position(CastlingMode::Standard)
        .context("failed to parse FEN into position")?;
    let hero = pos.turn();
    let legal_moves = pos.legal_moves();

    let mut sf = Stockfish::new(stockfish_path)?;
    sf.configure()?;

    let root_multipv = sf.analyze_root(fen, DEPTH_MAIN, MULTIPV_K)?;
    let mut candidates = Vec::new();
    let progress = ProgressBar::new(legal_moves.len() as u64);
    if let Ok(style) =
        ProgressStyle::with_template("[{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} {msg}")
    {
        progress.set_style(style.progress_chars("##-"));
    }
    progress.set_message("analyzing legal moves");

    for mv in legal_moves.iter() {
        let move_uci = move_to_uci(mv)?;
        let mut eval_by_depth = Vec::new();
        for depth in DEPTHS_STABILITY {
            let data = sf.analyze_searchmove(fen, depth, &move_uci)?;
            let (score_pawns, mate_in) = normalize_score_to_white(pos.turn(), data.cp, data.mate);
            let pv_uci = data.pv.into_iter().take(PV_MAX_PLIES).collect::<Vec<_>>();
            let pv_san = pv_to_san(&pos, &pv_uci)?;
            eval_by_depth.push(EvalByDepth {
                depth,
                score_pawns,
                mate_in,
                pv_uci,
                pv_san,
            });
        }

        let stability = compute_stability(&eval_by_depth);
        let tactical_flags = tactical_flags(&pos, mv)?;
        let forcing = forcing_counts(&pos, eval_by_depth.iter().find(|x| x.depth == DEPTH_MAIN));

        candidates.push(CandidateOutput {
            move_uci,
            move_san: San::from_move(&pos, mv).to_string(),
            eval_by_depth,
            stability,
            tactical_flags,
            pv_forcing_counts_first_12plies: forcing,
        });
        progress.inc(1);
    }
    progress.finish_with_message("analysis complete");

    Ok(AnalyzerOutput {
        schema_version: "1.0".to_string(),
        position: PositionInfo {
            fen: fen.to_string(),
            hero: color_to_str(hero).to_string(),
        },
        analysis_constants: AnalysisConstants {
            depths_stability: DEPTHS_STABILITY,
            depth_main: DEPTH_MAIN,
            pv_max_plies: PV_MAX_PLIES,
            multipv_k: MULTIPV_K,
        },
        multipv_from_root: MultipvRoot {
            depth: DEPTH_MAIN,
            lines: root_multipv
                .into_iter()
                .map(|l| {
                    let (score_pawns, mate_in) = normalize_score_to_white(pos.turn(), l.cp, l.mate);
                    MultipvLine {
                        rank: l.multipv,
                        move_uci: l.pv.first().cloned().unwrap_or_default(),
                        score_pawns,
                        mate_in,
                    }
                })
                .collect(),
        },
        candidates,
    })
}

fn normalize_score_to_white(
    side_to_move_for_analysis: Color,
    cp: Option<i32>,
    mate: Option<i32>,
) -> (Option<f64>, Option<i32>) {
    let perspective = if side_to_move_for_analysis == Color::White {
        1.0
    } else {
        -1.0
    };
    let mate_sign = if side_to_move_for_analysis == Color::White {
        1
    } else {
        -1
    };
    let score_pawns = cp.map(|v| round_two_decimals((v as f64 / 100.0) * perspective));
    let mate_in = mate.map(|v| v * mate_sign);
    (score_pawns, mate_in)
}

fn round_two_decimals(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn tactical_flags(pos: &Chess, mv: &Move) -> Result<TacticalFlags> {
    let is_capture = pos.board().occupied().contains(mv.to())
        && pos.board().color_at(mv.to()) != Some(pos.turn());
    let is_promotion = mv.promotion().is_some();

    let mut next = pos.clone();
    next.play_unchecked(mv);
    let is_check = next.is_check();

    Ok(TacticalFlags {
        candidate_is_check: is_check,
        candidate_is_capture: is_capture,
        candidate_is_promotion: is_promotion,
    })
}

fn forcing_counts(pos: &Chess, eval: Option<&EvalByDepth>) -> ForcingCounts {
    let Some(eval) = eval else {
        return ForcingCounts::default();
    };
    let mut board = pos.clone();
    let mut out = ForcingCounts::default();
    for uci in &eval.pv_uci {
        if let Ok(ucimv) = uci.parse::<UciMove>() {
            if let Ok(mv) = ucimv.to_move(&board) {
                let is_capture = board.board().occupied().contains(mv.to())
                    && board.board().color_at(mv.to()) != Some(board.turn());
                if is_capture {
                    out.captures += 1;
                }
                if mv.promotion().is_some() {
                    out.promotions += 1;
                }
                board.play_unchecked(&mv);
                if board.is_check() {
                    out.checks += 1;
                }
            }
        }
    }
    out
}

fn compute_stability(eval_by_depth: &[EvalByDepth]) -> Stability {
    let cp_values = eval_by_depth
        .iter()
        .filter_map(|e| e.score_pawns)
        .collect::<Vec<_>>();
    let has_mate = eval_by_depth.iter().any(|e| e.mate_in.is_some());

    let (range_pawns, mean_pawns, std_pawns) = if cp_values.len() >= 2 {
        let min = cp_values.iter().copied().fold(f64::INFINITY, f64::min);
        let max = cp_values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let mean = cp_values.iter().sum::<f64>() / cp_values.len() as f64;
        let var = cp_values
            .iter()
            .map(|v| {
                let d = *v - mean;
                d * d
            })
            .sum::<f64>()
            / cp_values.len() as f64;
        (
            Some(round_two_decimals(max - min)),
            Some(round_two_decimals(mean)),
            Some(round_two_decimals(var.sqrt())),
        )
    } else {
        (None, None, None)
    };

    let pvs = eval_by_depth
        .iter()
        .map(|e| e.pv_uci.clone())
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>();
    let prefix = common_prefix_len(&pvs);
    let main_len = eval_by_depth
        .iter()
        .find(|e| e.depth == DEPTH_MAIN)
        .map(|e| e.pv_uci.len())
        .unwrap_or(0);
    let ratio = if main_len == 0 {
        0.0
    } else {
        prefix as f64 / main_len as f64
    };

    Stability {
        has_mate,
        range_pawns,
        mean_pawns,
        std_pawns,
        pv_common_prefix_plies: prefix,
        pv_stability_ratio: ratio,
    }
}

fn common_prefix_len(lines: &[Vec<String>]) -> usize {
    if lines.is_empty() {
        return 0;
    }
    let shortest = lines.iter().map(|l| l.len()).min().unwrap_or(0);
    for idx in 0..shortest {
        let mv = &lines[0][idx];
        if lines.iter().skip(1).any(|line| &line[idx] != mv) {
            return idx;
        }
    }
    shortest
}

fn pv_to_san(pos: &Chess, pv_uci: &[String]) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut board = pos.clone();
    for uci in pv_uci {
        let ucimv: UciMove = uci
            .parse()
            .with_context(|| format!("invalid UCI move in PV: {uci}"))?;
        let mv = ucimv
            .to_move(&board)
            .with_context(|| format!("illegal PV move: {uci}"))?;
        out.push(San::from_move(&board, &mv).to_string());
        board.play_unchecked(&mv);
    }
    Ok(out)
}

fn move_to_uci(mv: &Move) -> Result<String> {
    Ok(UciMove::from_move(mv, shakmaty::CastlingMode::Standard).to_string())
}

fn color_to_str(color: Color) -> &'static str {
    if color == Color::White { "w" } else { "b" }
}

struct Stockfish {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl Stockfish {
    fn new(path: &Path) -> Result<Self> {
        let mut child = Command::new(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("failed to launch stockfish at {}", path.display()))?;

        let input = child.stdin.take().context("missing stdin for stockfish")?;
        let output = BufReader::new(
            child
                .stdout
                .take()
                .context("missing stdout for stockfish")?,
        );
        let mut sf = Self {
            child,
            input,
            output,
        };
        sf.send("uci")?;
        sf.wait_for("uciok")?;
        sf.send("isready")?;
        sf.wait_for("readyok")?;
        Ok(sf)
    }

    fn configure(&mut self) -> Result<()> {
        self.send("setoption name Threads value 1")?;
        self.send("setoption name Hash value 128")?;
        self.send("setoption name UCI_AnalyseMode value true")?;
        self.send("setoption name Contempt value 0")?;
        self.send("isready")?;
        self.wait_for("readyok")
    }

    fn analyze_root(&mut self, fen: &str, depth: u32, multipv: usize) -> Result<Vec<ParsedInfo>> {
        self.send(&format!("setoption name MultiPV value {multipv}"))?;
        self.send(&format!("position fen {fen}"))?;
        self.send(&format!("go depth {depth}"))?;
        let infos = self.collect_until_bestmove()?;
        let mut grouped = HashMap::<usize, ParsedInfo>::new();
        for info in infos {
            if info.depth == Some(depth) && !info.pv.is_empty() {
                grouped.insert(info.multipv, info);
            }
        }
        let mut out = grouped.into_values().collect::<Vec<_>>();
        out.sort_by_key(|x| x.multipv);
        Ok(out)
    }

    fn analyze_searchmove(&mut self, fen: &str, depth: u32, uci_move: &str) -> Result<ParsedInfo> {
        self.send("setoption name MultiPV value 1")?;
        self.send(&format!("position fen {fen}"))?;
        self.send(&format!("go depth {depth} searchmoves {uci_move}"))?;
        let infos = self.collect_until_bestmove()?;
        infos
            .into_iter()
            .filter(|i| i.depth == Some(depth) && !i.pv.is_empty())
            .last()
            .context("no analysis line returned for constrained search")
    }

    fn send(&mut self, cmd: &str) -> Result<()> {
        writeln!(self.input, "{cmd}")?;
        self.input.flush()?;
        Ok(())
    }

    fn wait_for(&mut self, marker: &str) -> Result<()> {
        let mut line = String::new();
        loop {
            line.clear();
            let n = self.output.read_line(&mut line)?;
            if n == 0 {
                bail!("stockfish closed unexpectedly while waiting for {marker}");
            }
            if line.trim() == marker {
                return Ok(());
            }
        }
    }

    fn collect_until_bestmove(&mut self) -> Result<Vec<ParsedInfo>> {
        let mut out = Vec::new();
        let mut line = String::new();
        loop {
            line.clear();
            let n = self.output.read_line(&mut line)?;
            if n == 0 {
                bail!("stockfish closed unexpectedly during analysis");
            }
            let trimmed = line.trim();
            if trimmed.starts_with("info ") {
                if let Some(info) = parse_info_line(trimmed) {
                    out.push(info);
                }
            }
            if trimmed.starts_with("bestmove ") {
                break;
            }
        }
        Ok(out)
    }
}

impl Drop for Stockfish {
    fn drop(&mut self) {
        let _ = self.send("quit");
        let _ = self.child.wait();
    }
}

fn parse_info_line(line: &str) -> Option<ParsedInfo> {
    let tokens = line.split_whitespace().collect::<Vec<_>>();
    let mut depth = None;
    let mut multipv = 1usize;
    let mut cp = None;
    let mut mate = None;
    let mut pv = Vec::new();

    let mut i = 0;
    while i < tokens.len() {
        match tokens[i] {
            "depth" => {
                i += 1;
                depth = tokens.get(i).and_then(|x| x.parse().ok());
            }
            "multipv" => {
                i += 1;
                multipv = tokens.get(i).and_then(|x| x.parse().ok()).unwrap_or(1);
            }
            "score" => {
                if i + 2 < tokens.len() {
                    match tokens[i + 1] {
                        "cp" => cp = tokens[i + 2].parse().ok(),
                        "mate" => mate = tokens[i + 2].parse().ok(),
                        _ => {}
                    }
                    i += 2;
                }
            }
            "pv" => {
                pv = tokens.iter().skip(i + 1).map(|s| s.to_string()).collect();
                break;
            }
            _ => {}
        }
        i += 1;
    }

    Some(ParsedInfo {
        depth,
        multipv,
        cp,
        mate,
        pv,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_info_line_cp() {
        let line = "info depth 20 multipv 2 score cp -37 nodes 100 pv e2e4 e7e5";
        let parsed = parse_info_line(line).unwrap();
        assert_eq!(parsed.depth, Some(20));
        assert_eq!(parsed.multipv, 2);
        assert_eq!(parsed.cp, Some(-37));
        assert_eq!(parsed.pv[0], "e2e4");
    }

    #[test]
    fn parses_info_line_mate() {
        let line = "info depth 20 score mate 3 pv e2e4";
        let parsed = parse_info_line(line).unwrap();
        assert_eq!(parsed.mate, Some(3));
        assert_eq!(parsed.cp, None);
    }

    #[test]
    fn common_prefix_is_correct() {
        let pvs = vec![
            vec!["e2e4".to_string(), "e7e5".to_string(), "g1f3".to_string()],
            vec!["e2e4".to_string(), "e7e5".to_string(), "b1c3".to_string()],
            vec!["e2e4".to_string(), "e7e5".to_string()],
        ];
        assert_eq!(common_prefix_len(&pvs), 2);
    }

    #[test]
    fn stability_metrics() {
        let evals = vec![
            EvalByDepth {
                depth: 12,
                score_pawns: Some(0.10),
                mate_in: None,
                pv_uci: vec!["e2e4".into()],
                pv_san: vec![],
            },
            EvalByDepth {
                depth: 16,
                score_pawns: Some(0.30),
                mate_in: None,
                pv_uci: vec!["e2e4".into()],
                pv_san: vec![],
            },
            EvalByDepth {
                depth: 20,
                score_pawns: Some(0.20),
                mate_in: None,
                pv_uci: vec!["e2e4".into()],
                pv_san: vec![],
            },
        ];
        let s = compute_stability(&evals);
        assert!((s.range_pawns.unwrap() - 0.20).abs() < 0.0001);
        assert!((s.mean_pawns.unwrap() - 0.20).abs() < 0.0001);
    }

    #[test]
    fn default_stockfish_path_has_expected_filename() {
        let p = default_stockfish_path();
        let fname = p.file_name().unwrap().to_string_lossy().to_string();
        #[cfg(target_os = "windows")]
        assert_eq!(fname, "stockfish.exe");
        #[cfg(not(target_os = "windows"))]
        assert_eq!(fname, "stockfish");
    }
}
