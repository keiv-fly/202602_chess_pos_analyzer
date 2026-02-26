use std::path::PathBuf;

use chess_pos_analyzer::{analyze_fen, default_stockfish_path};

#[test]
fn integration_real_stockfish_generates_json() {
    let stockfish = default_stockfish_path();
    if !stockfish.exists() {
        panic!(
            "stockfish executable not found at {}. Run scripts/download_stockfish.sh first.",
            stockfish.display()
        );
    }

    let fen = "r2q1rk1/p3ppbp/2pp1np1/4n3/4P3/1PN3PP/P4PB1/R1BQ1RK1 b - - 0 14";
    let output = analyze_fen(fen, &PathBuf::from(&stockfish)).expect("analysis should succeed");

    assert_eq!(output.schema_version, "1.0");
    assert_eq!(output.position.hero, "b");
    assert!(!output.multipv_from_root.lines.is_empty());
    assert!(!output.candidates.is_empty());

    let json = serde_json::to_value(&output).expect("serialization works");
    assert!(json.get("candidates").is_some());
}
