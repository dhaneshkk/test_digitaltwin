-- ══════════════════════════════════════════════════════════════
-- Digital Twin Engine — SQLite Schema v3
-- ══════════════════════════════════════════════════════════════

CREATE TABLE IF NOT EXISTS ontologies (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL,
    version         TEXT NOT NULL DEFAULT '1.0.0',
    schema_version  TEXT NOT NULL DEFAULT '3.0.0',
    max_timesteps   INTEGER NOT NULL DEFAULT 10,
    fingerprint     TEXT NOT NULL,
    spec_json       TEXT NOT NULL,  -- JSON blob
    created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE(name, version)
);

CREATE TABLE IF NOT EXISTS simulation_runs (
    id              TEXT PRIMARY KEY,
    ontology_id     TEXT NOT NULL REFERENCES ontologies(id) ON DELETE CASCADE,
    run_label       TEXT NOT NULL DEFAULT '',
    action_sequence TEXT NOT NULL,  -- JSON array
    has_model       INTEGER NOT NULL DEFAULT 0,
    n_successes     INTEGER NOT NULL DEFAULT 0,
    n_errors        INTEGER NOT NULL DEFAULT 0,
    grounding_us    INTEGER NOT NULL DEFAULT 0,  -- microseconds
    solve_us        INTEGER NOT NULL DEFAULT 0,
    timeline        TEXT,           -- JSON
    friction_warnings TEXT,         -- JSON
    created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_runs_ontology ON simulation_runs(ontology_id, created_at DESC);

CREATE TABLE IF NOT EXISTS audit_entries (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id          TEXT NOT NULL REFERENCES simulation_runs(id) ON DELETE CASCADE,
    ontology_id     TEXT NOT NULL REFERENCES ontologies(id) ON DELETE CASCADE,
    timestep        INTEGER NOT NULL,
    user_id         TEXT NOT NULL,
    action          TEXT NOT NULL,
    target          TEXT NOT NULL,
    outcome         TEXT NOT NULL,
    detail          TEXT NOT NULL DEFAULT '',
    chain_hash      TEXT NOT NULL,
    created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_audit_run     ON audit_entries(run_id, timestep);
CREATE INDEX IF NOT EXISTS idx_audit_user    ON audit_entries(ontology_id, user_id);
CREATE INDEX IF NOT EXISTS idx_audit_outcome ON audit_entries(outcome);

CREATE TABLE IF NOT EXISTS sam_analyses (
    id                   TEXT PRIMARY KEY,
    ontology_id          TEXT NOT NULL REFERENCES ontologies(id) ON DELETE CASCADE,
    base_sequence        TEXT NOT NULL,
    n_perturbations      INTEGER NOT NULL,
    mcc                  REAL NOT NULL,
    mcc_ci_lo            REAL NOT NULL,
    mcc_ci_hi            REAL NOT NULL,
    cohens_kappa         REAL NOT NULL,
    accuracy             REAL NOT NULL,
    tp INTEGER NOT NULL, tn INTEGER NOT NULL,
    fp INTEGER NOT NULL, fn INTEGER NOT NULL,
    perturbation_details TEXT NOT NULL,
    created_at           TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE IF NOT EXISTS validation_reports (
    id              TEXT PRIMARY KEY,
    ontology_id     TEXT NOT NULL REFERENCES ontologies(id) ON DELETE CASCADE,
    is_valid        INTEGER NOT NULL,
    errors          TEXT NOT NULL DEFAULT '[]',
    warnings        TEXT NOT NULL DEFAULT '[]',
    created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
