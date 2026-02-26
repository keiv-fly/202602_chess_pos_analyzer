# chess_pos_analyzer

CLI tool that takes a FEN and analyzes every legal move with Stockfish, outputting a JSON report.

## Requirements

- Rust toolchain with Cargo
- Stockfish executable in `./stockfish`
  - Linux x86_64 shortcut: `./scripts/download_stockfish.sh`
  - Windows/macOS: place executable manually in `stockfish/stockfish.exe` (Windows) or `stockfish/stockfish` (macOS/Linux), or pass `--stockfish <path>`.

## Run

```bash
cargo run -- --fen "r2q1rk1/p3ppbp/2pp1np1/4n3/4P3/1PN3PP/P4PB1/R1BQ1RK1 b - - 0 14"
```

## Tests

```bash
cargo test
cargo test --test integration_check
```

The integration test uses a real Stockfish engine.
