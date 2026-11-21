# 🏭 Digital Twin Engine v3

**Universal neuro-symbolic simulation engine** — pure Rust, Topcoat full-stack UI, SQLite backend.

Built on two frameworks for why 95% of GenAI pilots fail:
- **MIT/Forbes** — Engineer FOR friction, don't eliminate it
- **HBR/INSEAD** (Furr & Shipilov) — IFD framework for disciplined experimentation

```
┌─────────────────────┬──────────────────────────────────┐
│ MIT/Forbes Principle │ Rust Implementation              │
├─────────────────────┼──────────────────────────────────┤
│ Engineer friction    │ FrictionLayer (stateful PROD-05) │
│ Governance           │ SHA-256 chained audit trail      │
│ Memory / Context     │ SQLite persistent registry       │
│ Workflow redesign    │ Typed Rust structs (not strings) │
│ Learning from exp.   │ SAM perturbation analysis        │
│ Customer value       │ customer_impact field + IFD      │
│ Design for scale     │ OntologyValidator (7 checks)     │
│ Ninja teams / OCap   │ CapabilityMatrix                 │
└─────────────────────┴──────────────────────────────────┘
```

---

## Architecture

```
                  ┌──────────────────────────┐
                  │      Topcoat UI          │
                  │  (server-rendered pages) │
                  │   / and /dashboard/*     │
                  └───────────┬──────────────┘
                              │
              ┌───────────────┼───────────────┐
              │               │               │
     ┌────────▼──────┐  ┌────▼─────┐  ┌──────▼───────┐
     │  Axum JSON API │  │  Engine  │  │  Friction    │
     │  /api/*        │  │  (pure   │  │  Layer       │
     │  16 endpoints  │  │   Rust)  │  │  (stateful)  │
     └────────┬───────┘  └────┬─────┘  └──────────────┘
              │               │
         ┌────▼───────────────▼────┐
         │      SQLite (WAL)       │
         │   rusqlite (bundled)    │
         │  digital_twin.db       │
         └─────────────────────────┘
```

**Key design decision:** The ASP/Clingo solver from the Python version is replaced by a **pure Rust deterministic state machine** with frame axiom (inertia). This eliminates the C binding dependency entirely — the binary is fully self-contained.

---

## Quick Start

### Prerequisites

- Rust 1.85+ (edition 2024)
- No other dependencies — SQLite is bundled via `rusqlite`

### Build & Run

```bash
git clone https://github.com/dhaneshkk/test_digitaltwin.git
cd test_digitaltwin
cargo run
```

The server starts at `http://localhost:3000`:
- **UI** → `http://localhost:3000` (landing page)
- **Dashboard** → `http://localhost:3000/dashboard`
- **API** → `http://localhost:3000/api/health`

SQLite database `digital_twin.db` is created automatically on first run.

---

## Usage Guide

### 1. Create an Ontology

An ontology defines the domain — entities, properties, actions, capability tokens, and IFD scores.

```bash
curl -X POST http://localhost:3000/api/ontologies \
  -H 'Content-Type: application/json' \
  -d '{
  "spec": {
    "name": "Rail Fleet Digital Twin",
    "version": "3.0.0",
    "max_timesteps": 10,
    "concepts": ["train", "station", "technician"],
    "properties": {
      "status": { "allowed_values": ["operational", "broken", "maintenance"] },
      "location": { "allowed_values": ["depot", "track"] },
      "speed": { "allowed_values": ["0", "100"] }
    },
    "actions": {
      "report_fault": {
        "input_state": { "status": "operational" },
        "output_state": { "status": "broken" },
        "required_capability": "observer_token",
        "customer_impact": "Detects failures before service disruption",
        "intensity": "HIGH", "frequency": "HIGH", "density": "HIGH"
      },
      "move_to_depot": {
        "input_state": { "status": "broken", "location": "track" },
        "output_state": { "location": "depot" },
        "required_capability": "driver_token",
        "customer_impact": "Clears faulty trains from active lines",
        "intensity": "HIGH", "frequency": "MEDIUM", "density": "MEDIUM"
      },
      "fix_machine": {
        "input_state": { "status": "broken", "location": "depot" },
        "output_state": { "status": "operational" },
        "required_capability": "repair_token",
        "customer_impact": "Restores fleet capacity for timetable reliability",
        "intensity": "HIGH", "frequency": "MEDIUM", "density": "HIGH"
      }
    },
    "initial_entities": [
      { "entity_id": "train_1", "properties": { "status": "operational", "location": "track", "speed": "100" } },
      { "entity_id": "train_2", "properties": { "status": "operational", "location": "track", "speed": "100" } },
      { "entity_id": "train_3", "properties": { "status": "broken", "location": "depot", "speed": "0" } }
    ],
    "capability_grants": [
      { "user_id": "user_gordon", "token": "observer_token" },
      { "user_id": "user_dhanesh", "token": "repair_token" },
      { "user_id": "user_dhanesh", "token": "driver_token" },
      { "user_id": "user_sarah", "token": "observer_token" },
      { "user_id": "user_sarah", "token": "driver_token" }
    ]
  }
}'
```

Response:
```json
{ "ok": true, "data": { "id": "<uuid>", "fingerprint": "40ae216e96c2b6d6" } }
```

### 2. Validate the Ontology

Runs 7 checks: property references, capability coverage, value linkage (HBR), initial state consistency, reachability, deadlock detection, token economy audit.

```bash
curl -X POST http://localhost:3000/api/ontologies/<id>/validate
```

```json
{
  "ok": true,
  "data": {
    "is_valid": true,
    "errors": [],
    "warnings": []
  }
}
```

### 3. View IFD Priority Matrix

Furr & Shipilov's Intensity/Frequency/Density scoring — determines which actions to pilot first.

```bash
curl http://localhost:3000/api/ontologies/<id>/ifd
```

```json
{
  "ok": true,
  "data": [
    {
      "action_name": "report_fault",
      "ifd_score": 9,
      "priority": "CRITICAL",
      "customer_impact": "Detects failures before service disruption"
    }
  ]
}
```

### 4. Run a Simulation

Submit an action sequence — the engine evaluates each step against the state machine with capability checks, state preconditions, and frame axiom (inertia).

```bash
# Happy path: full repair workflow
curl -X POST http://localhost:3000/api/ontologies/<id>/simulate \
  -H 'Content-Type: application/json' \
  -d '{
  "label": "Full repair workflow",
  "actions": [
    ["user_gordon", "report_fault", "train_1"],
    ["user_dhanesh", "move_to_depot", "train_1"],
    ["user_dhanesh", "fix_machine", "train_1"]
  ]
}'
```

```json
{
  "ok": true,
  "data": {
    "run_id": "<uuid>",
    "has_model": true,
    "successes": [
      { "t": 0, "action": "report_fault" },
      { "t": 1, "action": "move_to_depot" },
      { "t": 2, "action": "fix_machine" }
    ],
    "errors": [],
    "friction_warnings": [],
    "grounding_us": 12,
    "solve_us": 45
  }
}
```

**Error scenario — unauthorized action:**

```bash
curl -X POST http://localhost:3000/api/ontologies/<id>/simulate \
  -H 'Content-Type: application/json' \
  -d '{
  "label": "Unauthorized repair attempt",
  "actions": [
    ["user_gordon", "report_fault", "train_1"],
    ["user_gordon", "fix_machine", "train_1"]
  ]
}'
```

The friction layer catches `user_gordon` lacking `repair_token` before the state machine even evaluates.

### 5. Verify Audit Chain Integrity

Every action is logged with a SHA-256 hash chain. Each entry's hash includes the previous entry's hash — any tampering breaks the chain.

```bash
curl -X POST http://localhost:3000/api/ontologies/<id>/audit/verify
```

```json
{ "ok": true, "data": { "valid": true, "entries_checked": 42 } }
```

### 6. View User Statistics & Failure Patterns

MIT's "learning gap" — detect repeated failures across runs.

```bash
curl http://localhost:3000/api/ontologies/<id>/audit/stats
```

```json
{
  "ok": true,
  "data": {
    "user_stats": [
      { "user_id": "user_dhanesh", "total": 20, "successes": 18, "success_rate": 90.0 }
    ],
    "failure_patterns": [
      { "action": "fix_machine", "outcome": "error_capability", "detail": "missing cap repair_token", "count": 5 }
    ]
  }
}
```

### 7. Run SAM Robustness Analysis

Sharpness-Aware perturbation testing — generates combinatorial perturbations (unknown users, bad actions, phantom entities, reversed order, swaps, omissions, empty sequence) and measures MCC with bootstrap 95% CI.

```bash
curl -X POST http://localhost:3000/api/ontologies/<id>/sam \
  -H 'Content-Type: application/json' \
  -d '{
  "actions": [
    ["user_gordon", "report_fault", "train_1"],
    ["user_dhanesh", "move_to_depot", "train_1"],
    ["user_dhanesh", "fix_machine", "train_1"]
  ],
  "n_bootstrap": 500
}'
```

```json
{
  "ok": true,
  "data": {
    "mcc": 1.0,
    "mcc_ci": [1.0, 1.0],
    "cohens_kappa": 1.0,
    "accuracy": 1.0,
    "confusion": { "tp": 1, "tn": 19, "fp": 0, "fn": 0 },
    "n_perturbations": 20,
    "details": [
      { "label": "Baseline", "expected_clean": true, "actual_clean": true },
      { "label": "Unknown user T=0", "expected_clean": false, "actual_clean": false }
    ]
  }
}
```

**MCC interpretation:**
| MCC | Meaning |
|-----|---------|
| > 0.5 | Robust ✅ |
| 0 – 0.5 | Needs hardening ⚠️ |
| ≤ 0 | Fragile ❌ |

---

## API Reference

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/api/health` | Health check (DB connectivity) |
| `POST` | `/api/ontologies` | Create ontology |
| `GET` | `/api/ontologies` | List all ontologies |
| `GET` | `/api/ontologies/:id` | Get ontology (full spec) |
| `DELETE` | `/api/ontologies/:id` | Delete ontology (cascades) |
| `POST` | `/api/ontologies/:id/validate` | Run 7-check validation |
| `GET` | `/api/ontologies/:id/ifd` | IFD priority matrix |
| `GET` | `/api/ontologies/:id/caps` | Capability matrix |
| `POST` | `/api/ontologies/:id/simulate` | Run simulation |
| `GET` | `/api/ontologies/:id/runs` | List simulation runs |
| `GET` | `/api/runs/:id` | Get run detail + timeline |
| `GET` | `/api/ontologies/:id/audit` | Audit trail entries |
| `POST` | `/api/ontologies/:id/audit/verify` | Verify SHA-256 chain |
| `GET` | `/api/ontologies/:id/audit/stats` | User stats + failure patterns |
| `POST` | `/api/ontologies/:id/sam` | Run SAM robustness analysis |
| `GET` | `/api/ontologies/:id/sam` | List SAM analysis history |

All responses follow the envelope: `{ "ok": true/false, "data": ..., "error": "..." }`

---

## Project Structure

```
├── Cargo.toml                     # Dependencies
├── migrations/
│   └── 001_init.sql               # SQLite schema (5 tables)
└── src/
    ├── main.rs                    # Entry — Topcoat + Axum on :3000
    ├── models.rs                  # Typed domain structs (258 lines)
    │   ├── IfdScore               #   LOW/MEDIUM/HIGH enum
    │   ├── ActionDef              #   input/output state + IFD scores
    │   ├── BusinessOntology       #   Complete ontology container
    │   ├── SimulationResult       #   Timeline + successes/errors
    │   └── SamResult              #   MCC + confusion matrix + CI
    ├── engine.rs                  # Core logic (506 lines)
    │   ├── validate_ontology()    #   7-check validator
    │   ├── friction_check()       #   Stateful pre-flight (PROD-05)
    │   ├── simulate()             #   Pure Rust state machine
    │   ├── compute_chain_hash()   #   SHA-256 audit chain
    │   └── sam_analysis()         #   Combinatorial perturbations + MCC
    ├── db.rs                      # SQLite persistence (375 lines)
    │   ├── insert/get/list/delete #   Ontology CRUD
    │   ├── insert_run/list_runs   #   Simulation run persistence
    │   ├── audit trail ops        #   Insert, verify chain, stats
    │   └── SAM ops                #   Insert + list analyses
    ├── api.rs                     # Axum JSON routes (352 lines)
    │   └── 16 endpoint handlers   #   All REST operations
    └── pages/
        ├── mod.rs
        ├── app.rs                 # Landing page (Topcoat view!)
        └── dashboard.rs           # Full dashboard (Topcoat view!)
            ├── Ontology selector  #   Sidebar with all ontologies
            ├── Stats row          #   Entities / Actions / Users / Timesteps
            ├── Validation panel   #   Errors + warnings
            ├── IFD matrix table   #   Color-coded priority
            ├── Capability matrix  #   User → token → action cards
            ├── Entity cards       #   Initial state grid
            ├── Simulation runs    #   History with badges
            ├── Audit trail        #   SHA-256 chained entries
            ├── User statistics    #   Success rate per user
            └── SAM history        #   MCC trend across analyses
```

---

## Production Hardening Tags

| Tag | What It Does |
|-----|-------------|
| **PROD-01** | ASP-style identifier validation (atoms + integers) |
| **PROD-04** | SHA-256 integrity chain on audit trail |
| **PROD-05** | Stateful friction layer (simulates state across timesteps) |
| **PROD-06** | Deadlock detection + token economy audit in validator |
| **PROD-07** | Combinatorial SAM perturbation generator (20+ perturbations) |
| **PROD-08** | Full confusion matrix + bootstrap MCC confidence interval |
| **PROD-11** | Multi-entity fleet scenarios |
| **PROD-12** | Over-privileged user detection |

---

## Migration from Python

| Python (Flask + PostgreSQL) | Rust (Topcoat + SQLite) |
|---|---|
| `clingo` (C binding) | Pure Rust state machine |
| `psycopg2` + PostgreSQL | `rusqlite` bundled SQLite |
| Flask JSON routes | Axum router + `AppState` |
| React JSX dashboard | Topcoat `view!` + Tailwind |
| Python dataclasses | Rust structs + `serde` |
| `numpy` for MCC bootstrap | `rand` crate for bootstrap sampling |

---

## Theoretical Foundations

- **MIT/Forbes** (Jason Snyder): *"95% of GenAI Pilots Fail Because Companies Avoid Friction"*
- **HBR/INSEAD** (Nathan Furr & Andrew Shipilov): *"Beware the AI Experimentation Trap"* — IFD Framework
- **SAM**: Forstner et al. 2021 — Sharpness-Aware Minimization applied as ontology robustness testing
- **MCC**: Matthews 1975 — Matthews Correlation Coefficient for imbalanced binary classification
- **OCap**: Object-Capability security model — token-gated actions

## License

MIT
