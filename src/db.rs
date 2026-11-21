//! SQLite persistence layer via rusqlite.

use crate::models::*;
use rusqlite::{params, Connection, Result as SqlResult};
use std::path::Path;
use std::sync::{Arc, Mutex};

pub type Db = Arc<Mutex<Connection>>;

pub fn init_db(path: &str) -> SqlResult<Db> {
    let conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
    let schema = include_str!("../migrations/001_init.sql");
    conn.execute_batch(schema)?;
    Ok(Arc::new(Mutex::new(conn)))
}

// ── Ontology CRUD ────────────────────────────────────────────

pub fn insert_ontology(db: &Db, id: &str, onto: &BusinessOntology) -> SqlResult<()> {
    let conn = db.lock().unwrap();
    let spec = serde_json::to_string(onto).unwrap();
    conn.execute(
        "INSERT INTO ontologies (id, name, version, max_timesteps, fingerprint, spec_json)
         VALUES (?1,?2,?3,?4,?5,?6)",
        params![id, onto.name, onto.version, onto.max_timesteps, onto.fingerprint(), spec],
    )?;
    Ok(())
}

pub fn list_ontologies(db: &Db) -> SqlResult<Vec<serde_json::Value>> {
    let conn = db.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT o.id, o.name, o.version, o.fingerprint, o.max_timesteps, o.created_at,
                (SELECT COUNT(*) FROM simulation_runs r WHERE r.ontology_id = o.id) AS n_runs
         FROM ontologies o ORDER BY o.created_at DESC"
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(serde_json::json!({
            "id": row.get::<_, String>(0)?,
            "name": row.get::<_, String>(1)?,
            "version": row.get::<_, String>(2)?,
            "fingerprint": row.get::<_, String>(3)?,
            "max_timesteps": row.get::<_, i64>(4)?,
            "created_at": row.get::<_, String>(5)?,
            "n_runs": row.get::<_, i64>(6)?,
        }))
    })?;
    rows.collect()
}

pub fn get_ontology(db: &Db, id: &str) -> SqlResult<Option<BusinessOntology>> {
    let conn = db.lock().unwrap();
    let mut stmt = conn.prepare("SELECT spec_json FROM ontologies WHERE id = ?1")?;
    let result = stmt.query_row(params![id], |row| {
        let json: String = row.get(0)?;
        Ok(serde_json::from_str(&json).unwrap())
    });
    match result {
        Ok(onto) => Ok(Some(onto)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e),
    }
}

pub fn get_ontology_full(db: &Db, id: &str) -> SqlResult<Option<serde_json::Value>> {
    let conn = db.lock().unwrap();
    let mut stmt = conn.prepare("SELECT * FROM ontologies WHERE id = ?1")?;
    let result = stmt.query_row(params![id], |row| {
        let spec_str: String = row.get::<_, String>(6)?;
        let spec: serde_json::Value = serde_json::from_str(&spec_str).unwrap_or_default();
        Ok(serde_json::json!({
            "id": row.get::<_, String>(0)?,
            "name": row.get::<_, String>(1)?,
            "version": row.get::<_, String>(2)?,
            "schema_version": row.get::<_, String>(3)?,
            "max_timesteps": row.get::<_, i64>(4)?,
            "fingerprint": row.get::<_, String>(5)?,
            "spec_json": spec,
            "created_at": row.get::<_, String>(7)?,
        }))
    });
    match result {
        Ok(v) => Ok(Some(v)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e),
    }
}

pub fn delete_ontology(db: &Db, id: &str) -> SqlResult<bool> {
    let conn = db.lock().unwrap();
    let n = conn.execute("DELETE FROM ontologies WHERE id = ?1", params![id])?;
    Ok(n > 0)
}

// ── Simulation Runs ──────────────────────────────────────────

pub fn insert_run(
    db: &Db,
    run_id: &str,
    ontology_id: &str,
    label: &str,
    result: &SimulationResult,
    action_seq: &[ActionStep],
) -> SqlResult<()> {
    let conn = db.lock().unwrap();
    conn.execute(
        "INSERT INTO simulation_runs
         (id, ontology_id, run_label, action_sequence, has_model,
          n_successes, n_errors, grounding_us, solve_us, timeline, friction_warnings)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![
            run_id, ontology_id, label,
            serde_json::to_string(action_seq).unwrap(),
            result.has_model as i32,
            result.successes.len() as i64,
            result.errors.len() as i64,
            result.grounding_us as i64,
            result.solve_us as i64,
            serde_json::to_string(&result.timeline).unwrap(),
            serde_json::to_string(&result.friction_warnings).unwrap(),
        ],
    )?;
    Ok(())
}

pub fn list_runs(db: &Db, ontology_id: &str) -> SqlResult<Vec<serde_json::Value>> {
    let conn = db.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT id, run_label, has_model, n_successes, n_errors,
                grounding_us, solve_us, created_at
         FROM simulation_runs WHERE ontology_id = ?1
         ORDER BY created_at DESC LIMIT 50"
    )?;
    let rows = stmt.query_map(params![ontology_id], |row| {
        Ok(serde_json::json!({
            "id": row.get::<_, String>(0)?,
            "run_label": row.get::<_, String>(1)?,
            "has_model": row.get::<_, i32>(2)? != 0,
            "n_successes": row.get::<_, i64>(3)?,
            "n_errors": row.get::<_, i64>(4)?,
            "grounding_us": row.get::<_, i64>(5)?,
            "solve_us": row.get::<_, i64>(6)?,
            "created_at": row.get::<_, String>(7)?,
        }))
    })?;
    rows.collect()
}

pub fn get_run(db: &Db, run_id: &str) -> SqlResult<Option<serde_json::Value>> {
    let conn = db.lock().unwrap();
    let mut stmt = conn.prepare("SELECT * FROM simulation_runs WHERE id = ?1")?;
    let result = stmt.query_row(params![run_id], |row| {
        let timeline_str: String = row.get::<_, Option<String>>(9)?.unwrap_or_default();
        let friction_str: String = row.get::<_, Option<String>>(10)?.unwrap_or_default();
        Ok(serde_json::json!({
            "id": row.get::<_, String>(0)?,
            "ontology_id": row.get::<_, String>(1)?,
            "run_label": row.get::<_, String>(2)?,
            "action_sequence": serde_json::from_str::<serde_json::Value>(
                &row.get::<_, String>(3)?
            ).unwrap_or_default(),
            "has_model": row.get::<_, i32>(4)? != 0,
            "n_successes": row.get::<_, i64>(5)?,
            "n_errors": row.get::<_, i64>(6)?,
            "grounding_us": row.get::<_, i64>(7)?,
            "solve_us": row.get::<_, i64>(8)?,
            "timeline": serde_json::from_str::<serde_json::Value>(&timeline_str)
                .unwrap_or_default(),
            "friction_warnings": serde_json::from_str::<serde_json::Value>(&friction_str)
                .unwrap_or_default(),
            "created_at": row.get::<_, String>(11)?,
        }))
    });
    match result {
        Ok(v) => Ok(Some(v)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e),
    }
}

// ── Audit Trail ──────────────────────────────────────────────

pub fn insert_audit_entry(db: &Db, entry: &AuditEntry) -> SqlResult<()> {
    let conn = db.lock().unwrap();
    conn.execute(
        "INSERT INTO audit_entries
         (run_id, ontology_id, timestep, user_id, action, target, outcome, detail, chain_hash)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![
            entry.run_id, entry.ontology_id, entry.timestep as i64,
            entry.user_id, entry.action, entry.target,
            entry.outcome, entry.detail, entry.chain_hash,
        ],
    )?;
    Ok(())
}

pub fn get_last_chain_hash(db: &Db, ontology_id: &str) -> SqlResult<String> {
    let conn = db.lock().unwrap();
    let result = conn.query_row(
        "SELECT chain_hash FROM audit_entries WHERE ontology_id = ?1 ORDER BY id DESC LIMIT 1",
        params![ontology_id],
        |row| row.get::<_, String>(0),
    );
    match result {
        Ok(h) => Ok(h),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(String::new()),
        Err(e) => Err(e),
    }
}

pub fn list_audit(db: &Db, ontology_id: &str, limit: usize) -> SqlResult<Vec<serde_json::Value>> {
    let conn = db.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT id, run_id, timestep, user_id, action, target, outcome, detail, chain_hash, created_at
         FROM audit_entries WHERE ontology_id = ?1 ORDER BY id DESC LIMIT ?2"
    )?;
    let rows = stmt.query_map(params![ontology_id, limit as i64], |row| {
        Ok(serde_json::json!({
            "id": row.get::<_, i64>(0)?,
            "run_id": row.get::<_, String>(1)?,
            "timestep": row.get::<_, i64>(2)?,
            "user_id": row.get::<_, String>(3)?,
            "action": row.get::<_, String>(4)?,
            "target": row.get::<_, String>(5)?,
            "outcome": row.get::<_, String>(6)?,
            "detail": row.get::<_, String>(7)?,
            "chain_hash": row.get::<_, String>(8)?,
            "created_at": row.get::<_, String>(9)?,
        }))
    })?;
    rows.collect()
}

pub fn verify_audit_chain(db: &Db, ontology_id: &str) -> SqlResult<(bool, usize)> {
    let conn = db.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT timestep, user_id, action, target, outcome, detail, chain_hash
         FROM audit_entries WHERE ontology_id = ?1 ORDER BY id"
    )?;
    let mut prev_hash = String::new();
    let mut count = 0usize;
    let rows = stmt.query_map(params![ontology_id], |row| {
        Ok((
            row.get::<_, i64>(0)? as usize,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, String>(6)?,
        ))
    })?;
    for row in rows {
        let (timestep, user, action, target, outcome, detail, stored_hash) = row?;
        let expected = crate::engine::compute_chain_hash(
            timestep, &user, &action, &target, &outcome, &detail, &prev_hash,
        );
        if stored_hash != expected {
            return Ok((false, count));
        }
        prev_hash = stored_hash;
        count += 1;
    }
    Ok((true, count))
}

pub fn user_stats(db: &Db, ontology_id: &str) -> SqlResult<Vec<UserStat>> {
    let conn = db.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT user_id,
                COUNT(*) as total,
                SUM(CASE WHEN outcome='success' THEN 1 ELSE 0 END) as successes
         FROM audit_entries WHERE ontology_id = ?1
         GROUP BY user_id"
    )?;
    let rows = stmt.query_map(params![ontology_id], |row| {
        let total: usize = row.get::<_, i64>(1)? as usize;
        let successes: usize = row.get::<_, i64>(2)? as usize;
        Ok(UserStat {
            user_id: row.get(0)?,
            total,
            successes,
            failures: total - successes,
            success_rate: if total > 0 { successes as f64 / total as f64 * 100.0 } else { 0.0 },
        })
    })?;
    rows.collect()
}

pub fn failure_patterns(db: &Db, ontology_id: &str) -> SqlResult<Vec<FailurePattern>> {
    let conn = db.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT action, outcome, detail, COUNT(*) as cnt
         FROM audit_entries WHERE ontology_id = ?1 AND outcome != 'success'
         GROUP BY action, outcome, detail HAVING cnt > 1 ORDER BY cnt DESC"
    )?;
    let rows = stmt.query_map(params![ontology_id], |row| {
        Ok(FailurePattern {
            action: row.get(0)?,
            outcome: row.get(1)?,
            detail: row.get(2)?,
            count: row.get::<_, i64>(3)? as usize,
        })
    })?;
    rows.collect()
}

// ── SAM ──────────────────────────────────────────────────────

pub fn insert_sam(db: &Db, ontology_id: &str, base_seq: &[ActionStep], result: &SamResult) -> SqlResult<()> {
    let conn = db.lock().unwrap();
    conn.execute(
        "INSERT INTO sam_analyses
         (id, ontology_id, base_sequence, n_perturbations, mcc, mcc_ci_lo, mcc_ci_hi,
          cohens_kappa, accuracy, tp, tn, fp, fn, perturbation_details)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
        params![
            result.id, ontology_id,
            serde_json::to_string(base_seq).unwrap(),
            result.n_perturbations as i64,
            result.mcc, result.mcc_ci.0, result.mcc_ci.1,
            result.cohens_kappa, result.accuracy,
            result.confusion.tp as i64, result.confusion.tn as i64,
            result.confusion.fp as i64, result.confusion.r#fn as i64,
            serde_json::to_string(&result.details).unwrap(),
        ],
    )?;
    Ok(())
}

pub fn list_sam(db: &Db, ontology_id: &str) -> SqlResult<Vec<serde_json::Value>> {
    let conn = db.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT id, mcc, mcc_ci_lo, mcc_ci_hi, cohens_kappa, accuracy,
                n_perturbations, tp, tn, fp, fn, created_at
         FROM sam_analyses WHERE ontology_id = ?1 ORDER BY created_at DESC"
    )?;
    let rows = stmt.query_map(params![ontology_id], |row| {
        Ok(serde_json::json!({
            "id": row.get::<_, String>(0)?,
            "mcc": row.get::<_, f64>(1)?,
            "mcc_ci_lo": row.get::<_, f64>(2)?,
            "mcc_ci_hi": row.get::<_, f64>(3)?,
            "cohens_kappa": row.get::<_, f64>(4)?,
            "accuracy": row.get::<_, f64>(5)?,
            "n_perturbations": row.get::<_, i64>(6)?,
            "tp": row.get::<_, i64>(7)?,
            "tn": row.get::<_, i64>(8)?,
            "fp": row.get::<_, i64>(9)?,
            "fn": row.get::<_, i64>(10)?,
            "created_at": row.get::<_, String>(11)?,
        }))
    })?;
    rows.collect()
}

// ── Validation ───────────────────────────────────────────────

pub fn insert_validation(db: &Db, ontology_id: &str, report: &ValidationReport) -> SqlResult<()> {
    let conn = db.lock().unwrap();
    conn.execute(
        "INSERT INTO validation_reports (id, ontology_id, is_valid, errors, warnings)
         VALUES (?1,?2,?3,?4,?5)",
        params![
            uuid::Uuid::new_v4().to_string(),
            ontology_id,
            report.is_valid as i32,
            serde_json::to_string(&report.errors).unwrap(),
            serde_json::to_string(&report.warnings).unwrap(),
        ],
    )?;
    Ok(())
}
