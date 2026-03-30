//! Simulated annealing solver for QUBO matching problems.
//!
//! Pure Rust implementation of simulated annealing for the
//! donor-recipient assignment problem. Orders of magnitude faster
//! than Python (dimod) for large problem sizes.
//!
//! In production: replaced by D-Wave quantum annealing hardware.
//! This simulator demonstrates the same QUBO formulation that
//! runs on quantum hardware.

use crate::scoring::{self, BloodType};

/// QUBO problem: minimize x^T Q x where x is binary.
pub struct QuboProblem {
    /// Number of binary variables
    pub n_vars: usize,
    /// Linear biases (diagonal of Q)
    pub linear: Vec<f64>,
    /// Quadratic interactions (off-diagonal): (i, j, value)
    pub quadratic: Vec<(usize, usize, f64)>,
    /// Variable labels for interpretation
    pub labels: Vec<(usize, usize)>, // (donor_idx, recipient_idx)
}

/// Solution to a QUBO problem.
pub struct QuboSolution {
    pub assignment: Vec<bool>,
    pub energy: f64,
    pub pairs: Vec<(usize, usize)>,
}

/// Donor data for QUBO formulation.
#[derive(Debug, Clone)]
pub struct QuboDonor {
    pub blood_type: BloodType,
    pub liver_volume: f64,
    pub region_km: f64,
}

/// Recipient data for QUBO formulation.
#[derive(Debug, Clone)]
pub struct QuboRecipient {
    pub blood_type: BloodType,
    pub meld_score: f64,
    pub body_weight: f64,
    pub region_km: f64,
    pub waiting_days: f64,
}

/// Build score matrix from donors and recipients.
pub fn build_score_matrix(donors: &[QuboDonor], recipients: &[QuboRecipient]) -> Vec<Vec<f64>> {
    let max_wait = recipients.iter()
        .map(|r| r.waiting_days)
        .fold(1.0_f64, f64::max);

    donors.iter().map(|d| {
        recipients.iter().map(|r| {
            let abo = scoring::abo_compatibility(d.blood_type, r.blood_type);
            let meld = scoring::meld_priority(r.meld_score);
            let grwr_val = d.liver_volume / r.body_weight / 10.0;
            let grwr = if grwr_val < 0.8 || grwr_val > 5.0 {
                0.0
            } else {
                (1.0 - (grwr_val - 2.0).abs() / 3.0).max(0.0)
            };
            let dist = (d.region_km - r.region_km).abs();
            let isch = scoring::ischemia_score(dist);
            let wait = scoring::waiting_time_priority(r.waiting_days, max_wait);
            scoring::composite_score(abo, meld, grwr, isch, wait)
        }).collect()
    }).collect()
}

/// Build QUBO from score matrix.
pub fn build_qubo(scores: &[Vec<f64>], penalty: f64) -> QuboProblem {
    let nd = scores.len();
    let nr = if nd > 0 { scores[0].len() } else { 0 };

    let mut linear = Vec::new();
    let mut quadratic = Vec::new();
    let mut labels = Vec::new();
    let mut var_map: Vec<(usize, usize)> = Vec::new();

    // Create variables only for non-zero score pairs
    for d in 0..nd {
        for r in 0..nr {
            if scores[d][r] > 0.0 {
                linear.push(-scores[d][r]); // minimize negative = maximize
                labels.push((d, r));
                var_map.push((d, r));
            }
        }
    }

    let n_vars = linear.len();

    // Constraint: each donor matched to at most 1 recipient
    for i in 0..n_vars {
        for j in (i + 1)..n_vars {
            if var_map[i].0 == var_map[j].0 {
                // Same donor
                quadratic.push((i, j, penalty));
            }
        }
    }

    // Constraint: each recipient matched to at most 1 donor
    for i in 0..n_vars {
        for j in (i + 1)..n_vars {
            if var_map[i].1 == var_map[j].1 {
                // Same recipient
                quadratic.push((i, j, penalty));
            }
        }
    }

    QuboProblem { n_vars, linear, quadratic, labels }
}

/// Simulated annealing solver.
///
/// Parameters calibrated for matching problems.
/// In production: this is replaced by D-Wave QPU.
pub fn simulated_annealing(
    problem: &QuboProblem,
    n_sweeps: usize,
    t_init: f64,
    t_final: f64,
    seed: u64,
) -> QuboSolution {
    let n = problem.n_vars;
    if n == 0 {
        return QuboSolution { assignment: vec![], energy: 0.0, pairs: vec![] };
    }

    // Simple LCG random number generator (no external dependency)
    let mut rng_state = seed;
    let mut next_f64 = || -> f64 {
        rng_state = rng_state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (rng_state >> 33) as f64 / (1u64 << 31) as f64
    };

    // Initialize random solution
    let mut state: Vec<bool> = (0..n).map(|_| next_f64() < 0.3).collect();

    // Compute initial energy
    let compute_energy = |s: &[bool]| -> f64 {
        let mut e = 0.0;
        for i in 0..n {
            if s[i] {
                e += problem.linear[i];
            }
        }
        for &(i, j, val) in &problem.quadratic {
            if s[i] && s[j] {
                e += val;
            }
        }
        e
    };

    let mut energy = compute_energy(&state);
    let mut best_state = state.clone();
    let mut best_energy = energy;

    // Build adjacency list for fast energy delta computation
    let mut neighbors: Vec<Vec<(usize, f64)>> = vec![vec![]; n];
    for &(i, j, val) in &problem.quadratic {
        neighbors[i].push((j, val));
        neighbors[j].push((i, val));
    }

    // Annealing schedule
    let t_ratio = (t_final / t_init).ln();

    for sweep in 0..n_sweeps {
        let progress = sweep as f64 / n_sweeps as f64;
        let temp = t_init * (t_ratio * progress).exp();

        for _ in 0..n {
            let flip = (next_f64() * n as f64) as usize % n;

            // Compute energy delta for flipping variable `flip`
            let mut delta = if state[flip] {
                -problem.linear[flip]
            } else {
                problem.linear[flip]
            };

            for &(neighbor, val) in &neighbors[flip] {
                if state[neighbor] {
                    delta += if state[flip] { -val } else { val };
                }
            }

            // Metropolis criterion
            if delta < 0.0 || next_f64() < (-delta / temp).exp() {
                state[flip] = !state[flip];
                energy += delta;

                if energy < best_energy {
                    best_energy = energy;
                    best_state = state.clone();
                }
            }
        }
    }

    // Extract pairs from best solution
    let pairs: Vec<(usize, usize)> = best_state.iter()
        .enumerate()
        .filter(|(_, &v)| v)
        .map(|(i, _)| problem.labels[i])
        .collect();

    QuboSolution {
        assignment: best_state,
        energy: best_energy,
        pairs,
    }
}

/// Greedy matching (baseline comparison).
pub fn greedy_match(scores: &[Vec<f64>]) -> Vec<(usize, usize, f64)> {
    crate::matching::greedy_match(scores)
}

/// Run full comparison: greedy vs simulated annealing.
pub fn compare_methods(
    donors: &[QuboDonor],
    recipients: &[QuboRecipient],
) -> ComparisonResult {
    let scores = build_score_matrix(donors, recipients);
    let n = donors.len();

    // Greedy
    let t0 = std::time::Instant::now();
    let greedy_result = greedy_match(&scores);
    let greedy_time = t0.elapsed();
    let greedy_score: f64 = greedy_result.iter().map(|&(_, _, s)| s).sum();

    // Simulated annealing (quantum simulator)
    let qubo = build_qubo(&scores, 10.0);
    let t0 = std::time::Instant::now();
    let sa_result = simulated_annealing(&qubo, 1000, 10.0, 0.01, 42);
    let sa_time = t0.elapsed();
    let sa_score: f64 = sa_result.pairs.iter()
        .map(|&(d, r)| scores[d][r])
        .sum();

    let improvement = if greedy_score > 0.0 {
        (sa_score - greedy_score) / greedy_score * 100.0
    } else {
        0.0
    };

    ComparisonResult {
        n,
        n_variables: qubo.n_vars,
        n_interactions: qubo.quadratic.len(),
        greedy_matches: greedy_result.len(),
        greedy_score,
        greedy_time_ms: greedy_time.as_secs_f64() * 1000.0,
        quantum_matches: sa_result.pairs.len(),
        quantum_score: sa_score,
        quantum_time_ms: sa_time.as_secs_f64() * 1000.0,
        improvement_pct: improvement,
    }
}

#[derive(Debug, Clone)]
pub struct ComparisonResult {
    pub n: usize,
    pub n_variables: usize,
    pub n_interactions: usize,
    pub greedy_matches: usize,
    pub greedy_score: f64,
    pub greedy_time_ms: f64,
    pub quantum_matches: usize,
    pub quantum_score: f64,
    pub quantum_time_ms: f64,
    pub improvement_pct: f64,
}

impl std::fmt::Display for ComparisonResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "n={:>4} vars={:>5} ints={:>6} | \
            greedy: {:>3}match {:.2}score {:.1}ms | \
            quantum: {:>3}match {:.2}score {:.1}ms | \
            Δ={:+.1}%",
            self.n, self.n_variables, self.n_interactions,
            self.greedy_matches, self.greedy_score, self.greedy_time_ms,
            self.quantum_matches, self.quantum_score, self.quantum_time_ms,
            self.improvement_pct)
    }
}

/// Generate random scenario for benchmarking.
pub fn generate_scenario(n: usize, seed: u64) -> (Vec<QuboDonor>, Vec<QuboRecipient>) {
    let mut rng = seed;
    let mut next_rand = || -> f64 {
        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (rng >> 33) as f64 / (1u64 << 31) as f64
    };

    let pick_bt = |r: f64| -> BloodType {
        if r < 0.30 { BloodType::O }
        else if r < 0.70 { BloodType::A }
        else if r < 0.90 { BloodType::B }
        else { BloodType::AB }
    };

    let locations = [0.0, 100.0, 250.0, 400.0, 550.0, 700.0, 850.0, 1000.0];

    let mut donors: Vec<QuboDonor> = Vec::with_capacity(n);
    for _ in 0..n {
        let bt = pick_bt(next_rand());
        let vol = 1200.0 + next_rand() * 600.0;
        let loc = locations[(next_rand() * 8.0) as usize % 8] + (next_rand() - 0.5) * 100.0;
        donors.push(QuboDonor { blood_type: bt, liver_volume: vol, region_km: loc });
    }

    let mut recipients: Vec<QuboRecipient> = Vec::with_capacity(n);
    for _ in 0..n {
        let bt = pick_bt(next_rand());
        let meld = 10.0 + next_rand() * 30.0;
        let bw = 45.0 + next_rand() * 40.0;
        let loc = locations[(next_rand() * 8.0) as usize % 8] + (next_rand() - 0.5) * 100.0;
        let wait = next_rand() * 1500.0;
        recipients.push(QuboRecipient { blood_type: bt, meld_score: meld, body_weight: bw, region_km: loc, waiting_days: wait });
    }

    (donors, recipients)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_small_matching() {
        let (donors, recipients) = generate_scenario(5, 42);
        let result = compare_methods(&donors, &recipients);
        println!("{}", result);
        assert!(result.quantum_matches > 0);
        assert!(result.quantum_score > 0.0);
    }

    #[test]
    fn test_medium_matching() {
        let (donors, recipients) = generate_scenario(20, 42);
        let result = compare_methods(&donors, &recipients);
        println!("{}", result);
        assert!(result.quantum_matches >= result.greedy_matches - 1);
    }

    #[test]
    fn test_large_matching() {
        let (donors, recipients) = generate_scenario(50, 42);
        let result = compare_methods(&donors, &recipients);
        println!("{}", result);
        assert!(result.quantum_time_ms < 60000.0); // should complete in < 60s
    }

    #[test]
    fn test_scale_100() {
        let (donors, recipients) = generate_scenario(100, 42);
        let result = compare_methods(&donors, &recipients);
        println!("{}", result);
        assert!(result.quantum_matches > 0);
    }

    #[test]
    fn test_qubo_energy_negative_for_valid_match() {
        let (donors, recipients) = generate_scenario(5, 42);
        let scores = build_score_matrix(&donors, &recipients);
        let qubo = build_qubo(&scores, 10.0);
        let solution = simulated_annealing(&qubo, 500, 10.0, 0.01, 42);
        // Energy should be negative (maximizing scores = minimizing negative scores)
        assert!(solution.energy < 0.0);
    }
}
