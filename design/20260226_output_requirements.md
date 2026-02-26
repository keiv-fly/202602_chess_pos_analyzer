## Global definitions (used everywhere)

### Sides

* `hero` = side to move in the **original** position (the side choosing the candidate move).
* `opp` = the other side.

### Piece values (for “material in pawns”)

Use integer pawn units:

* `P=1, N=3, B=3, R=5, Q=9, K=0`

### Engine score normalization (always store from hero perspective)

Stockfish UCI `score` is **from the side-to-move perspective** of the position being analyzed.

Convert to `hero_score`:

* If `side_to_move == hero`: `hero_cp = cp`, `hero_mate = mate`
* Else: `hero_cp = -cp`, `hero_mate = -mate`

Store mate as `mate_in` (integer plies), and set `cp=null` when mate is present.

### Attack map (pseudo-attacks, not legality-filtered)

For any side, compute a map:

* `attackers_count[square] = number of pieces of that side that attack square`
  Attacks are generated with standard chess movement and current occupancy:
* Pawns: diagonals forward
* Knights: L-shape
* Bishops/Rooks/Queens: rays until first blocker (inclusive only of the attacked square, not beyond)
* King: adjacent squares
  **Do not remove attacks from pinned pieces.**

### Legal moves

Generate **fully legal** moves (king may not be left in check).

### PV parsing and move formats

* Store PV moves as **UCI** strings.
* Also store **SAN** for each PV move by applying the PV line on a board and generating SAN at each step (including `+`/`#`).

### Fixed analysis constants (strict)

Use exactly:

* `depths_stability = [12, 16, 20]`
* `depth_main = 20`
* `pv_max_plies = 12`
* `multipv_k = 6`
* `threat_depth = 16`
* `threat_multipv_k = 6`
* `threat_defense_threshold_cp = 30`  (0.30 pawns)

Engine options (fixed for determinism):

* `Threads=1`
* `Hash=128`
* `MultiPV=multipv_k` (only for the MultiPV call; otherwise `MultiPV=1`)
* `UCI_AnalyseMode=true`
* `Contempt=0` (or equivalent if present)

---

# 1) Stability of the evaluation

For each candidate move `m`:

### 1.1 Eval by depth (constrained to the move)

For each `d` in `depths_stability`:

1. Set position to original FEN.
2. Run Stockfish with `go depth d searchmoves m`.
3. Extract:

   * score: `cp` or `mate`
   * PV line (UCI moves)
4. Normalize score to hero perspective (rule above).
5. Truncate PV to `pv_max_plies`.

Store list `eval_by_depth[] = {depth, hero_cp, hero_mate_in, bound, pv_uci[]}`
`bound` is:

* `"exact"` if no `lowerbound/upperbound`
* `"lowerbound"` / `"upperbound"` if present in UCI output

### 1.2 Numerical stability metrics

Let `S` be the set of hero scores at the depths, expressed in cp:

* If any depth returns mate, treat it separately:

  * `stability.has_mate = true`
  * `stability.mate_depths = depths where mate seen`
  * For numeric volatility, use only cp depths; if none exist, omit numeric volatility.

If at least 2 cp values exist:

* `range_cp = max(cp) - min(cp)`
* `mean_cp = average(cp)`
* `std_cp = sqrt( average( (cp - mean_cp)^2 ) )`

### 1.3 PV stability

Let `PV_d` be the PV list at depth `d` (UCI moves, truncated).
Compute:

* `common_prefix_plies = length of the longest common prefix among PV_12, PV_16, PV_20`
  (only compare the depths that exist; if some PV missing, compare remaining)
* `pv_stability_ratio = common_prefix_plies / len(PV_depth_main)` (if len>0 else 0)

---

# 2) Tactical forcing content

Work in the position **after applying candidate move**: `pos_after`.

### 2.1 Forcing flags for the candidate move itself

* `is_check`: candidate gives check in `pos_after` (king of opp is in check)
* `is_capture`: candidate captures a piece (destination square had enemy piece)
* `is_promotion`: candidate is a promotion

### 2.2 Forcing density in the main PV

Use the PV from `depth_main` (the constrained one for this move).
For the first `pv_max_plies` plies:

* Apply each PV move one by one and count:

  * `pv_checks`: number of PV moves that give check
  * `pv_captures`: number of PV moves that capture
  * `pv_promotions`: number of PV moves that promote

### 2.3 Hanging and overloaded pieces after the move

In `pos_after`, compute attack maps for both sides.

For every non-king piece `p` on square `sq`:

* `attacked = enemy_attackers_count[sq]`
* `defended = friendly_attackers_count[sq]`

Define:

* `hanging` if `attacked > 0` and `defended == 0`
* `overloaded` if `attacked > defended`

Output lists for both sides with:
`{piece: "P|N|B|R|Q", square: "e4", attacked_by: int, defended_by: int}`

### 2.4 Pin detection (to the king) after the move

For each side `S`:

1. Find `king_sq` of side `S`.
2. For each enemy slider (`R`,`B`,`Q`), if aligned with `king_sq` on a rook-line (rank/file) or bishop-line (diagonal):

   * Trace squares between slider and king.
   * If exactly **one** piece lies between them, and it belongs to side `S`, that piece is `pinned_to_king` by that slider.

Output pinned pieces for hero and opp with:
`{pinned_piece: {piece, square}, by: {piece, square}, line: "file|rank|diag"}`

### 2.5 Fork heuristic (by the moved piece) after the move

Let `moved_piece` be the piece that just moved.
Compute squares it attacks in `pos_after` and count how many enemy pieces of value ≥ 3 (`N,B,R,Q`) are attacked.

* `forks_valuable = true` if count ≥ 2
  Output attacked targets list.

### 2.6 Discovered attack heuristic

Compare `pos_before` vs `pos_after`:
For each hero slider (`B`,`R`,`Q`) in `pos_before`:

* If the moved piece was on a ray between that slider and an enemy piece,
* and after moving, the ray becomes open and the slider attacks that enemy piece square,
  then record `discovered_attack`.

---

# 3) King safety

All computed in `pos_after`.

### 3.1 King zone squares

For a king at `k`:

* `king_zone` = `{k} ∪ all squares with Chebyshev distance ≤ 1 from k` (up to 9 squares)

### 3.2 Opponent checks available

Let it be opp to move in `pos_after`.

* Generate all **legal** moves for opp.
* `opp_check_moves = count(moves that give check to hero king)`

### 3.3 Attack and defense scores in the hero king zone

Compute:

* `opp_attack_score = Σ over squares z in hero_king_zone ( opp_attackers_count[z] )`
* `hero_defense_score = Σ over squares z in hero_king_zone ( hero_attackers_count[z] )`

(This is a pure count-based metric; no piece-value weighting.)

### 3.4 Open/semi-open files near the hero king

Let `kf` be the king’s file index (a=1..h=8). Consider files `{kf-1, kf, kf+1}` within 1..8.

For each such file `f`:

* `has_any_pawn(f)`: any pawn exists on file `f`
* `has_hero_pawn(f)`: hero pawn exists on file `f`

Count:

* `open_files_near_king = number of f with has_any_pawn(f)=false`
* `semi_open_files_near_king = number of f with has_any_pawn(f)=true AND has_hero_pawn(f)=false`

### 3.5 Pawn shield missing squares

Let `dir = +1` for White king shield (toward rank increasing), `dir = -1` for Black.
Define shield squares:

* Rank `r1 = king_rank + dir`
* Rank `r2 = king_rank + 2*dir`
* Files `kf-1,kf,kf+1` within board
  Valid shield squares are those on-board among those 6 squares.
* `pawn_shield_present = count(valid shield squares occupied by hero pawn)`
* `pawn_shield_missing = valid_count - pawn_shield_present`

### 3.6 Single king-safety index

Compute:
`king_safety_index = (opp_attack_score - hero_defense_score) + 2*open_files_near_king + 1*semi_open_files_near_king + 1*pawn_shield_missing + 2*opp_check_moves`

Higher means **worse** for hero.

---

# 4) Piece activity and coordination

Compute mobility for hero in both `pos_before` and `pos_after` using this strict rule:

### 4.1 “Hero mobility” (independent of side-to-move)

Create two cloned boards:

* `before_hero_turn`: same pieces as `pos_before`, but set `side_to_move = hero`
* `after_hero_turn`: same pieces as `pos_after`, but set `side_to_move = hero`

Generate **legal** moves for hero in each.
For each hero piece type `T`, count legal moves originating from squares containing `T`.

Define weighted mobility:
`weighted_mobility = Σ_T (piece_value(T) * moves_count(T))` using values in Global definitions.

Output:

* `weighted_mobility_before`
* `weighted_mobility_after`
* `weighted_mobility_delta = after - before`
* `mobility_by_piece_type_before/after` (P,N,B,R,Q,K)

### 4.2 Coordination flags (after the move)

In `pos_after`:

* `bishop_pair`: hero has two bishops
* `connected_rooks`: hero rooks on same rank/file with no pieces between
* `pieces_defended_count`: number of hero non-king pieces with `defended_by > 0` (using attack map)

---

# 5) Pawn structure and long-term plan

Compute in `pos_before` and `pos_after`, and store *created/removed* deltas.

### 5.1 Pawn sets

Represent each pawn by square string.

### 5.2 Isolated pawn

A hero pawn on file `f` is isolated if hero has **no** pawn on file `f-1` and **no** pawn on file `f+1`.

### 5.3 Doubled pawn

File `f` is doubled if hero has `count_pawns_on_file(f) >= 2`.
Mark all pawns on doubled files as doubled.

### 5.4 Passed pawn

A hero pawn on square `(f,r)` is passed if there are **no enemy pawns** on files `{f-1,f,f+1}` on any rank **ahead** of it:

* If hero is White: enemy pawn rank > r
* If hero is Black: enemy pawn rank < r

### 5.5 Backward pawn (strict heuristic)

A hero pawn at `(f,r)` is backward if all conditions hold:

1. It is **not passed**
2. There is **no hero pawn** on adjacent files `{f-1,f+1}` on a rank **≥ r** (White) or **≤ r** (Black)
3. The square one step forward `(f, r+dir)` exists and is attacked by at least one **enemy pawn**

### 5.6 Pawn islands

Sort hero pawn files that contain at least one pawn. Count contiguous groups of files with no gaps.
Example: pawns on files a,b,d,e => islands = 2 (ab) + (de).

### 5.7 Space (pawn-based)

Compute:

* `pawn_space`: number of squares in opponent half attacked by hero pawns
  Opponent half:
* if hero is White: ranks 5–8
* if hero is Black: ranks 1–4

### 5.8 Deltas

For each property (passed/isolated/doubled/backward, islands, pawn_space), compute:

* `before`, `after`, and created/removed pawn lists (where applicable)

---

# 6) Strategic targets and threats

All computed relative to hero.

### 6.1 Weak enemy pawns (after the move)

For each enemy pawn on square `sq` in `pos_after`:

* `attackers = hero_attackers_count[sq]`
* `defenders = opp_attackers_count[sq]`
  Define `weak_enemy_pawn` if:
* `attackers > defenders`
  AND
* `defended_by_enemy_pawn = true/false` (true if any enemy pawn attacks `sq`)
  Store weak pawns with counts and pawn-defense flag.

### 6.2 Holes created in opponent camp (pawn-control holes)

Compute hole squares for the opponent side in `pos_before` and `pos_after`.

Define a square `s` as a **hole for opponent** if:

1. `s` is in the opponent hole zone:

   * If opponent is White: ranks `{3,4,5}`
   * If opponent is Black: ranks `{4,5,6}`
2. `s` is **not attacked by any opponent pawn**

Compute:

* `holes_created = holes_after \ holes_before`
* `holes_removed = holes_before \ holes_after`

### 6.3 Threat/forcing indicator via opponent defensive freedom (engine-based)

This is computed with one extra engine call **per candidate**:

1. Set position to `pos_after` (opponent to move).
2. Run Stockfish with `go depth threat_depth` and `MultiPV=threat_multipv_k`.
3. For each defensive line `i` in 1..K, extract score and normalize to **hero perspective** (invert because side-to-move is opp).
4. Let `best = max(hero_score_i)` (higher is better for hero).
5. Count:

   * `defenses_within_threshold = number of i with hero_score_i >= best - threat_defense_threshold_cp`
6. Define:

   * `is_forcing_threat = (defenses_within_threshold == 1)`

Store the defensive moves and their scores.

---

# 7) Endgame / progression signals

### 7.1 Simplification tendency in PV

Using the constrained main PV (depth_main), first `pv_max_plies` plies:

* `captures_in_pv = number of capture moves`
* `queen_trade_in_pv = true` if both queens are captured at some point in that PV prefix
* `major_piece_captures_in_pv = captures where captured piece is R or Q`

### 7.2 Material imbalance summary (after the move)

Count pieces for both sides and store difference:

* `counts_hero` and `counts_opp` for P,N,B,R,Q
* `material_pawns_hero = Σ value(piece)*count`
* `material_pawns_opp = ...`
* `material_diff_pawns = hero - opp`

---

# 8) Human-readable record (single JSON per position)

This is the JSON structure you output. It includes:

* engine settings
* the MultiPV ranking from the original position
* one entry per candidate move, with every metric above

---

## Example JSON (illustrative values, structure is normative)

```json
{
  "schema_version": "1.0",
  "position": {
    "fen": "r2q1rk1/p3ppbp/2pp1np1/4n3/4P3/1PN3PP/P4PB1/R1BQ1RK1 b - - 0 14",
    "hero": "b",
    "move_number": 14
  },
  "analysis_constants": {
    "depths_stability": [12, 16, 20],
    "depth_main": 20,
    "pv_max_plies": 12,
    "multipv_k": 6,
    "threat_depth": 16,
    "threat_multipv_k": 6,
    "threat_defense_threshold_cp": 30,
    "piece_values_pawns": { "P": 1, "N": 3, "B": 3, "R": 5, "Q": 9, "K": 0 }
  },
  "engine": {
    "name": "Stockfish",
    "uci_options": {
      "Threads": 1,
      "Hash": 128,
      "UCI_AnalyseMode": true,
      "Contempt": 0
    }
  },
  "multipv_from_root": {
    "depth": 20,
    "lines": [
      { "rank": 1, "move_uci": "c6c5", "hero_score": { "type": "cp", "cp": 22, "mate_in": null, "bound": "exact" } },
      { "rank": 2, "move_uci": "b8d7", "hero_score": { "type": "cp", "cp": 10, "mate_in": null, "bound": "exact" } },
      { "rank": 3, "move_uci": "a7a5", "hero_score": { "type": "cp", "cp": 6, "mate_in": null, "bound": "exact" } },
      { "rank": 4, "move_uci": "d6d5", "hero_score": { "type": "cp", "cp": 2, "mate_in": null, "bound": "exact" } },
      { "rank": 5, "move_uci": "h7h5", "hero_score": { "type": "cp", "cp": -5, "mate_in": null, "bound": "exact" } },
      { "rank": 6, "move_uci": "c6c5", "hero_score": { "type": "cp", "cp": 22, "mate_in": null, "bound": "exact" } }
    ]
  },
  "candidates": [
    {
      "move_uci": "b8d7",
      "move_san": "Nd7",
      "root_rank_multipv": 2,
      "gap_to_best_cp": 12,

      "eval_by_depth": [
        {
          "depth": 12,
          "hero_score": { "type": "cp", "cp": 18, "mate_in": null, "bound": "exact" },
          "pv_uci": ["b8d7", "c1b2", "a7a5", "a1d1", "c6c5", "b2g7"],
          "pv_san": ["Nd7", "Bb2", "a5", "Rad1", "c5", "Bxg7"]
        },
        {
          "depth": 16,
          "hero_score": { "type": "cp", "cp": 12, "mate_in": null, "bound": "exact" },
          "pv_uci": ["b8d7", "c1b2", "c6c5", "b2g7", "c8e6", "f1d1"],
          "pv_san": ["Nd7", "Bb2", "c5", "Bxg7", "Be6", "Rd1"]
        },
        {
          "depth": 20,
          "hero_score": { "type": "cp", "cp": 10, "mate_in": null, "bound": "exact" },
          "pv_uci": ["b8d7", "c1b2", "c6c5", "b2g7", "c8e6", "f1d1", "g8g7", "d1d2"],
          "pv_san": ["Nd7", "Bb2", "c5", "Bxg7", "Be6", "Rd1", "Kxg7", "Rd2"]
        }
      ],

      "stability": {
        "has_mate": false,
        "range_cp": 8,
        "mean_cp": 13.3333333333,
        "std_cp": 3.3993463424,
        "pv_common_prefix_plies": 2,
        "pv_stability_ratio": 0.25
      },

      "tactics": {
        "candidate_is_check": false,
        "candidate_is_capture": false,
        "candidate_is_promotion": false,

        "pv_forcing_counts_first_12plies": {
          "checks": 0,
          "captures": 2,
          "promotions": 0
        },

        "hanging_after_move": {
          "hero_hanging": [
            { "piece": "N", "square": "d7", "attacked_by": 1, "defended_by": 0 }
          ],
          "opp_hanging": [
            { "piece": "B", "square": "g2", "attacked_by": 1, "defended_by": 0 }
          ]
        },

        "overloaded_after_move": {
          "hero_overloaded": [
            { "piece": "P", "square": "c6", "attacked_by": 2, "defended_by": 1 }
          ],
          "opp_overloaded": []
        },

        "pinned_to_king_after_move": {
          "hero_pinned": [],
          "opp_pinned": [
            {
              "pinned_piece": { "piece": "N", "square": "f3" },
              "by": { "piece": "B", "square": "g4" },
              "line": "diag"
            }
          ]
        },

        "forks_valuable_by_moved_piece": {
          "is_fork": false,
          "targets": []
        },

        "discovered_attacks_created": [
          {
            "slider": { "piece": "B", "square": "g7" },
            "target": { "piece": "R", "square": "a1" },
            "line": "diag"
          }
        ]
      },

      "king_safety": {
        "hero_king_square": "g8",
        "opp_check_moves_available": 1,
        "opp_attack_score_king_zone": 6,
        "hero_defense_score_king_zone": 4,
        "open_files_near_king": 0,
        "semi_open_files_near_king": 1,
        "pawn_shield_missing": 2,
        "king_safety_index": 7
      },

      "activity": {
        "hero_weighted_mobility_before": 112,
        "hero_weighted_mobility_after": 118,
        "hero_weighted_mobility_delta": 6,
        "mobility_by_piece_type": {
          "before": { "P": 12, "N": 6, "B": 7, "R": 4, "Q": 3, "K": 2 },
          "after":  { "P": 12, "N": 8, "B": 7, "R": 4, "Q": 3, "K": 2 }
        },
        "coordination_after": {
          "bishop_pair": true,
          "connected_rooks": false,
          "pieces_defended_count": 9
        }
      },

      "pawn_structure": {
        "hero": {
          "passed": { "before": [], "after": [], "created": [], "removed": [] },
          "isolated": { "before": ["a7"], "after": ["a7"], "created": [], "removed": [] },
          "doubled": { "before": [], "after": [], "created": [], "removed": [] },
          "backward": { "before": ["c6"], "after": ["c6"], "created": [], "removed": [] },
          "pawn_islands": { "before": 3, "after": 3, "delta": 0 },
          "pawn_space": { "before": 7, "after": 7, "delta": 0 }
        }
      },

      "targets_and_threats": {
        "weak_enemy_pawns_after": [
          { "square": "e4", "attackers": 2, "defenders": 1, "defended_by_enemy_pawn": false }
        ],
        "holes_created_in_opponent_camp": ["d4", "f4"],
        "holes_removed_in_opponent_camp": [],
        "opponent_defensive_freedom": {
          "depth": 16,
          "multipv_k": 6,
          "best_hero_cp": 14,
          "defenses_within_threshold": 1,
          "is_forcing_threat": true,
          "defensive_lines": [
            { "rank": 1, "move_uci": "c1b2", "hero_cp": 14 },
            { "rank": 2, "move_uci": "f1e1", "hero_cp": -25 },
            { "rank": 3, "move_uci": "b3b4", "hero_cp": -30 }
          ]
        }
      },

      "endgame": {
        "simplification_in_pv_first_12plies": {
          "captures": 2,
          "major_piece_captures": 0,
          "queen_trade": false
        },
        "material_after_move": {
          "counts_hero": { "P": 8, "N": 2, "B": 2, "R": 2, "Q": 1 },
          "counts_opp":  { "P": 7, "N": 2, "B": 2, "R": 2, "Q": 1 },
          "material_pawns_hero": 39,
          "material_pawns_opp": 38,
          "material_diff_pawns": 1
        }
      }
    }
  ]
}
```
