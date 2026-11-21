//! SAM-trained distress classifier [IGG-05] + MCC evaluation [IGG-06].
//! Logistic regression with Sharpness-Aware Minimization optimizer.
//! Bootstrap MCC with seeded RNG (seed=42) for reproducibility.

use crate::planner::TelemetryRecord;
use rand::{Rng, SeedableRng, rngs::StdRng};

// ── SAM Optimizer ────────────────────────────────────────────

struct SAMOptimizer {
    lr: f64,
    rho: f64,
    saved: Vec<f64>,
}

impl SAMOptimizer {
    fn new(lr: f64, rho: f64) -> Self {
        Self { lr, rho, saved: vec![] }
    }

    fn first_step(&mut self, params: &mut Vec<f64>, grads: &[f64]) {
        self.saved = params.clone();
        let norm: f64 = grads.iter().map(|g| g * g).sum::<f64>().sqrt().max(1e-12);
        for (p, g) in params.iter_mut().zip(grads) {
            *p += self.rho * g / norm;
        }
    }

    fn second_step(&self, params: &mut Vec<f64>, grads: &[f64]) {
        for (i, p) in params.iter_mut().enumerate() {
            *p = self.saved[i] - self.lr * grads[i];
        }
    }
}

// ── Distress Classifier ──────────────────────────────────────

pub struct DistressClassifier {
    w: Vec<f64>,
    b: f64,
    mu: Vec<f64>,
    sig: Vec<f64>,
    pub loss_curve: Vec<f64>,
}

impl DistressClassifier {
    pub fn new() -> Self {
        Self { w: vec![0.0; 4], b: 0.0, mu: vec![0.0; 4], sig: vec![1.0; 4], loss_curve: vec![] }
    }

    fn sigmoid(z: f64) -> f64 {
        1.0 / (1.0 + (-z.clamp(-30.0, 30.0)).exp())
    }

    fn normalise(&mut self, rows: &[[f64; 4]], fit: bool) -> Vec<Vec<f64>> {
        if fit {
            self.mu = (0..4).map(|j| rows.iter().map(|r| r[j]).sum::<f64>() / rows.len() as f64).collect();
            self.sig = (0..4).map(|j| {
                let m = self.mu[j];
                (rows.iter().map(|r| (r[j] - m).powi(2)).sum::<f64>() / rows.len() as f64)
                    .sqrt().max(1e-8)
            }).collect();
        }
        rows.iter().map(|r| (0..4).map(|j| (r[j] - self.mu[j]) / self.sig[j]).collect()).collect()
    }

    fn grads_loss(&self, xn: &[Vec<f64>], y: &[f64]) -> (Vec<f64>, f64, f64) {
        let n = xn.len() as f64;
        let (mut gw, mut gb, mut loss) = (vec![0.0f64; 4], 0.0f64, 0.0f64);
        for (xi, &yi) in xn.iter().zip(y) {
            let z: f64 = xi.iter().zip(&self.w).map(|(x, w)| x * w).sum::<f64>() + self.b;
            let p = Self::sigmoid(z);
            let e = p - yi;
            loss -= yi * (p + 1e-9).ln() + (1.0 - yi) * (1.0 - p + 1e-9).ln();
            gb += e;
            for (j, &x) in xi.iter().enumerate() {
                gw[j] += e * x;
            }
        }
        (gw.iter().map(|g| g / n).collect(), gb / n, loss / n)
    }

    pub fn fit(&mut self, recs: &[TelemetryRecord], epochs: usize, lr: f64, rho: f64) {
        if recs.len() < 4 { return; }
        let xs: Vec<[f64; 4]> = recs.iter().map(|r| r.features()).collect();
        let ys: Vec<f64> = recs.iter().map(|r| r.label as f64).collect();
        let xn = self.normalise(&xs, true);
        let mut params: Vec<f64> = self.w.iter().chain(std::iter::once(&self.b)).copied().collect();
        let mut sam = SAMOptimizer::new(lr, rho);

        for _ in 0..epochs {
            self.w.copy_from_slice(&params[..4]);
            self.b = params[4];
            let (mut gw1, gb1, _) = self.grads_loss(&xn, &ys);
            gw1.push(gb1);
            sam.first_step(&mut params, &gw1);

            self.w.copy_from_slice(&params[..4]);
            self.b = params[4];
            let (mut gw2, gb2, loss) = self.grads_loss(&xn, &ys);
            gw2.push(gb2);
            sam.second_step(&mut params, &gw2);

            self.w.copy_from_slice(&params[..4]);
            self.b = params[4];
            self.loss_curve.push(loss);
        }
    }

    pub fn predict_label(&self, rec: &TelemetryRecord) -> i32 {
        let f = rec.features();
        let xn: Vec<f64> = (0..4).map(|j| (f[j] - self.mu[j]) / self.sig[j]).collect();
        let z: f64 = xn.iter().zip(&self.w).map(|(x, w)| x * w).sum::<f64>() + self.b;
        if Self::sigmoid(z) >= 0.5 { 1 } else { 0 }
    }
}

// ── MCC Evaluation [IGG-06] ──────────────────────────────────

pub struct MCCResult {
    pub mcc: f64,
    pub ci_lo: f64,
    pub ci_hi: f64,
    pub tp: i64,
    pub tn: i64,
    pub fp: i64,
    pub fn_: i64,
    pub accuracy: f64,
    pub precision: f64,
    pub recall: f64,
    pub f1: f64,
    pub n: usize,
}

fn compute_mcc_raw(y: &[i32], p: &[i32]) -> f64 {
    let (mut tp, mut tn, mut fp, mut fn_) = (0i64, 0, 0, 0);
    for (&yi, &pi) in y.iter().zip(p) {
        match (yi, pi) {
            (1, 1) => tp += 1,
            (0, 0) => tn += 1,
            (0, 1) => fp += 1,
            _ => fn_ += 1,
        }
    }
    let d = ((tp + fp) * (tp + fn_) * (tn + fp) * (tn + fn_)) as f64;
    if d == 0.0 { 0.0 } else { (tp * tn - fp * fn_) as f64 / d.sqrt() }
}

pub fn evaluate_mcc(clf: &DistressClassifier, recs: &[TelemetryRecord]) -> MCCResult {
    let y: Vec<i32> = recs.iter().map(|r| r.label).collect();
    let p: Vec<i32> = recs.iter().map(|r| clf.predict_label(r)).collect();
    let mcc = compute_mcc_raw(&y, &p);
    let n = y.len();

    let (mut tp, mut tn, mut fp, mut fn_) = (0i64, 0, 0, 0);
    for (&yi, &pi) in y.iter().zip(&p) {
        match (yi, pi) {
            (1, 1) => tp += 1,
            (0, 0) => tn += 1,
            (0, 1) => fp += 1,
            _ => fn_ += 1,
        }
    }

    // Bootstrap MCC CI (seed=42 for reproducibility)
    let mut rng = StdRng::seed_from_u64(42);
    let mut boot: Vec<f64> = (0..500).map(|_| {
        let idx: Vec<usize> = (0..n).map(|_| rng.gen_range(0..n)).collect();
        compute_mcc_raw(
            &idx.iter().map(|&i| y[i]).collect::<Vec<_>>(),
            &idx.iter().map(|&i| p[i]).collect::<Vec<_>>(),
        )
    }).collect();
    boot.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let (ci_lo, ci_hi) = (boot[12], boot[487]);

    let acc = (tp + tn) as f64 / n as f64;
    let prec = if tp + fp > 0 { tp as f64 / (tp + fp) as f64 } else { 0.0 };
    let rec = if tp + fn_ > 0 { tp as f64 / (tp + fn_) as f64 } else { 0.0 };
    let f1 = if prec + rec > 0.0 { 2.0 * prec * rec / (prec + rec) } else { 0.0 };

    MCCResult { mcc, ci_lo, ci_hi, tp, tn, fp, fn_, accuracy: acc, precision: prec, recall: rec, f1, n }
}
