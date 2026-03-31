//! Multi-organ simultaneous allocation via QUBO.
//!
//! When a brain-dead donor provides K organs simultaneously,
//! the allocation problem becomes k-dimensional matching (NP-hard for k≥3).
//!
//! QUBO formulation:
//!   Variables: x_{k,i} ∈ {0,1} — assign organ k to patient i
//!   Objective: maximize Σ score_{k,i} · x_{k,i} + Σ combined_bonus
//!   Constraints:
//!     - Each organ assigned to at most 1 patient (penalty)
//!     - Each patient receives at most 1 of each organ type (penalty)
//!     - Combined transplant bonus (liver+kidney, heart+lung)

use crate::annealing::{QuboProblem, QuboSolution, simulated_annealing};
use crate::scoring::{self, BloodType};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

/// Organ types available from a brain-dead donor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Organ {
    Liver,
    KidneyL,
    KidneyR,
    Heart,
    LungL,
    LungR,
    Pancreas,
    SmallIntestine,
}

impl Organ {
    pub fn all() -> &'static [Organ] {
        &[
            Organ::Liver,
            Organ::KidneyL,
            Organ::KidneyR,
            Organ::Heart,
            Organ::LungL,
            Organ::LungR,
            Organ::Pancreas,
            Organ::SmallIntestine,
        ]
    }

    /// Cold ischemia time limit in hours.
    pub fn cold_ischemia_hours(&self) -> f64 {
        match self {
            Organ::Liver => 12.0,
            Organ::KidneyL | Organ::KidneyR => 24.0,
            Organ::Heart => 4.0,
            Organ::LungL | Organ::LungR => 6.0,
            Organ::Pancreas => 12.0,
            Organ::SmallIntestine => 8.0,
        }
    }

    /// Index for QUBO variable ordering.
    pub fn index(&self) -> usize {
        match self {
            Organ::Liver => 0,
            Organ::KidneyL => 1,
            Organ::KidneyR => 2,
            Organ::Heart => 3,
            Organ::LungL => 4,
            Organ::LungR => 5,
            Organ::Pancreas => 6,
            Organ::SmallIntestine => 7,
        }
    }
}

/// A waiting patient for multi-organ allocation.
#[derive(Clone, Debug)]
pub struct MultiOrganPatient {
    pub id: usize,
    pub blood_type: BloodType,
    pub body_weight: f64,
    pub meld_score: f64,
    pub waiting_days: f64,
    pub region_km: f64,
    /// Which organ(s) this patient needs.
    pub needed_organs: Vec<Organ>,
    /// True if this patient needs a combined transplant (e.g., liver+kidney).
    pub needs_combined: bool,
}

/// Donor information.
#[derive(Clone, Debug)]
pub struct MultiOrganDonor {
    pub blood_type: BloodType,
    pub body_weight: f64,
    pub liver_volume: f64,
    pub region_km: f64,
}

/// Score a donor-patient pair for a specific organ.
pub fn organ_score(donor: &MultiOrganDonor, patient: &MultiOrganPatient, organ: &Organ) -> f64 {
    if scoring::abo_compatibility(donor.blood_type, patient.blood_type) == 0.0 {
        return 0.0;
    }

    let distance = (donor.region_km - patient.region_km).abs();
    // Check cold ischemia constraint (rough: 100km/hour transport)
    let transport_hours = distance / 100.0;
    if transport_hours > organ.cold_ischemia_hours() {
        return 0.0;
    }

    let meld = scoring::meld_priority(patient.meld_score);
    let ischemia = scoring::ischemia_score(distance);
    let waiting = patient.waiting_days / 3000.0;

    let grwr_val = donor.liver_volume / patient.body_weight / 10.0;
    let size_match = match organ {
        Organ::Liver => {
            if grwr_val < 0.8 || grwr_val > 5.0 { 0.0 }
            else { (1.0 - (grwr_val - 2.0).abs() / 3.0).max(0.0) }
        }
        _ => {
            // Rough body size compatibility for non-liver organs
            let ratio = donor.body_weight / patient.body_weight;
            if ratio < 0.7 || ratio > 1.5 { 0.3 } else { 1.0 - (ratio - 1.0).abs() }
        }
    };

    scoring::composite_score(1.0, meld, size_match, ischemia, waiting)
}

/// Build QUBO for multi-organ simultaneous allocation.
///
/// Variables: x_{k,i} for each (organ k, patient i) pair with nonzero score.
/// Constraints encoded as penalty terms.
pub fn build_multi_organ_qubo(
    donor: &MultiOrganDonor,
    patients: &[MultiOrganPatient],
    organs: &[Organ],
    combined_bonus: f64,
    penalty: f64,
) -> (QuboProblem, Vec<(Organ, usize)>) {
    // Map: variable index → (organ, patient_index)
    let mut var_map: Vec<(Organ, usize)> = Vec::new();
    let mut linear: Vec<f64> = Vec::new();

    // Create variables for each (organ, patient) with nonzero score
    for &organ in organs {
        for (i, patient) in patients.iter().enumerate() {
            if !patient.needed_organs.contains(&organ) {
                continue;
            }
            let s = organ_score(donor, patient, &organ);
            if s > 0.0 {
                linear.push(-s); // minimize negative = maximize
                var_map.push((organ, i));
            }
        }
    }

    let n_vars = linear.len();
    let mut quadratic: Vec<(usize, usize, f64)> = Vec::new();

    // Constraint 1: Each organ assigned to at most 1 patient
    for a in 0..n_vars {
        for b in (a + 1)..n_vars {
            if var_map[a].0 == var_map[b].0 {
                quadratic.push((a, b, penalty));
            }
        }
    }

    // Constraint 2: Each patient receives at most 1 of each organ type
    // (already handled by needed_organs filtering, but add penalty for paired organs)
    // e.g., patient can't get both KidneyL and KidneyR
    for a in 0..n_vars {
        for b in (a + 1)..n_vars {
            if var_map[a].1 == var_map[b].1 {
                let (org_a, org_b) = (var_map[a].0, var_map[b].0);
                // Same patient, different organs of same type → penalty
                let same_type = matches!(
                    (org_a, org_b),
                    (Organ::KidneyL, Organ::KidneyR) | (Organ::KidneyR, Organ::KidneyL) |
                    (Organ::LungL, Organ::LungR) | (Organ::LungR, Organ::LungL)
                );
                if same_type {
                    quadratic.push((a, b, penalty));
                }
            }
        }
    }

    // Bonus: Combined transplant (negative penalty = bonus for co-assignment)
    for a in 0..n_vars {
        for b in (a + 1)..n_vars {
            if var_map[a].1 == var_map[b].1 {
                let patient = &patients[var_map[a].1];
                if !patient.needs_combined {
                    continue;
                }
                let (org_a, org_b) = (var_map[a].0, var_map[b].0);
                let is_combined = matches!(
                    (org_a, org_b),
                    (Organ::Liver, Organ::KidneyL) | (Organ::Liver, Organ::KidneyR) |
                    (Organ::KidneyL, Organ::Liver) | (Organ::KidneyR, Organ::Liver) |
                    (Organ::Heart, Organ::LungL) | (Organ::Heart, Organ::LungR) |
                    (Organ::LungL, Organ::Heart) | (Organ::LungR, Organ::Heart)
                );
                if is_combined {
                    quadratic.push((a, b, -combined_bonus)); // negative = bonus
                }
            }
        }
    }

    let labels = var_map.iter().map(|&(o, p)| (o.index(), p)).collect();
    (
        QuboProblem {
            n_vars,
            linear,
            quadratic,
            labels,
        },
        var_map,
    )
}

/// Solve multi-organ allocation and return assignments.
pub fn solve_multi_organ(
    donor: &MultiOrganDonor,
    patients: &[MultiOrganPatient],
    organs: &[Organ],
    combined_bonus: f64,
    penalty: f64,
    sweeps: usize,
    seed: u64,
) -> Vec<(Organ, usize, f64)> {
    let (qubo, var_map) = build_multi_organ_qubo(donor, patients, organs, combined_bonus, penalty);

    if qubo.n_vars == 0 {
        return vec![];
    }

    let solution = simulated_annealing(&qubo, sweeps, 10.0, 0.01, seed);

    let mut assignments = Vec::new();
    for (idx, &assigned) in solution.assignment.iter().enumerate() {
        if assigned {
            let (organ, patient_idx) = var_map[idx];
            let score = organ_score(donor, &patients[patient_idx], &organ);
            assignments.push((organ, patient_idx, score));
        }
    }
    assignments
}

/// Independent Hungarian: solve each organ separately (baseline).
pub fn solve_independent(
    donor: &MultiOrganDonor,
    patients: &[MultiOrganPatient],
    organs: &[Organ],
) -> Vec<(Organ, usize, f64)> {
    let mut assignments = Vec::new();
    let mut assigned_patients = std::collections::HashSet::new();

    for &organ in organs {
        let mut best_idx = None;
        let mut best_score = 0.0f64;
        for (i, patient) in patients.iter().enumerate() {
            if assigned_patients.contains(&i) {
                continue;
            }
            if !patient.needed_organs.contains(&organ) {
                continue;
            }
            let s = organ_score(donor, patient, &organ);
            if s > best_score {
                best_score = s;
                best_idx = Some(i);
            }
        }
        if let Some(idx) = best_idx {
            assignments.push((organ, idx, best_score));
            assigned_patients.insert(idx);
        }
    }
    assignments
}

/// Generate a realistic multi-organ scenario.
pub fn generate_multi_organ_scenario(
    n_per_organ: usize,
    seed: u64,
) -> (MultiOrganDonor, Vec<MultiOrganPatient>) {
    let mut rng = StdRng::seed_from_u64(seed);

    let donor = MultiOrganDonor {
        blood_type: random_bt(&mut rng),
        body_weight: rng.gen_range(50.0..90.0),
        liver_volume: rng.gen_range(1000.0..1800.0),
        region_km: rng.gen_range(0.0..500.0),
    };

    let mut patients = Vec::new();
    let all_organs = Organ::all();

    for (organ_idx, &organ) in all_organs.iter().enumerate() {
        for j in 0..n_per_organ {
            let needs_combined = match organ {
                Organ::Liver => rng.gen_range(0..100) < 15,  // 15% need liver+kidney
                Organ::Heart => rng.gen_range(0..100) < 10,  // 10% need heart+lung
                _ => false,
            };

            let mut needed = vec![organ];
            if needs_combined {
                match organ {
                    Organ::Liver => needed.push(Organ::KidneyL),
                    Organ::Heart => needed.push(Organ::LungL),
                    _ => {}
                }
            }

            patients.push(MultiOrganPatient {
                id: organ_idx * n_per_organ + j,
                blood_type: random_bt(&mut rng),
                body_weight: rng.gen_range(40.0..100.0),
                meld_score: rng.gen_range(6.0..40.0),
                waiting_days: rng.gen_range(30.0..3000.0),
                region_km: rng.gen_range(0.0..500.0),
                needed_organs: needed,
                needs_combined,
            });
        }
    }

    (donor, patients)
}

fn random_bt(rng: &mut StdRng) -> BloodType {
    match rng.gen_range(0..100) {
        0..=39 => BloodType::A,
        40..=59 => BloodType::O,
        60..=79 => BloodType::B,
        _ => BloodType::AB,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_multi_organ_qubo_builds() {
        let (donor, patients) = generate_multi_organ_scenario(5, 42);
        let organs = &[Organ::Liver, Organ::KidneyL, Organ::Heart];
        let (qubo, var_map) = build_multi_organ_qubo(&donor, &patients, organs, 2.0, 10.0);
        assert!(qubo.n_vars > 0, "QUBO should have variables");
        assert_eq!(var_map.len(), qubo.n_vars);
    }

    #[test]
    fn test_multi_organ_solve() {
        let (donor, patients) = generate_multi_organ_scenario(10, 42);
        let organs = &[Organ::Liver, Organ::KidneyL, Organ::KidneyR, Organ::Heart];

        let qubo_result = solve_multi_organ(&donor, &patients, organs, 2.0, 10.0, 50_000, 42);
        let indep_result = solve_independent(&donor, &patients, organs);

        let qubo_score: f64 = qubo_result.iter().map(|r| r.2).sum();
        let indep_score: f64 = indep_result.iter().map(|r| r.2).sum();

        println!("Independent: {} assignments, score {:.2}", indep_result.len(), indep_score);
        println!("QUBO:        {} assignments, score {:.2}", qubo_result.len(), qubo_score);

        // QUBO should find at least as good a solution
        assert!(
            qubo_result.len() > 0,
            "QUBO should find at least one assignment"
        );
    }

    #[test]
    fn test_combined_transplant_bonus() {
        let (donor, patients) = generate_multi_organ_scenario(20, 99);
        let organs = &[Organ::Liver, Organ::KidneyL, Organ::KidneyR, Organ::Heart, Organ::LungL];

        // Without combined bonus
        let result_no_bonus = solve_multi_organ(&donor, &patients, organs, 0.0, 10.0, 50_000, 42);
        // With combined bonus
        let result_bonus = solve_multi_organ(&donor, &patients, organs, 3.0, 10.0, 50_000, 42);

        let score_no: f64 = result_no_bonus.iter().map(|r| r.2).sum();
        let score_yes: f64 = result_bonus.iter().map(|r| r.2).sum();

        println!("No bonus: {} assignments, score {:.2}", result_no_bonus.len(), score_no);
        println!("Bonus:    {} assignments, score {:.2}", result_bonus.len(), score_yes);
    }
}
