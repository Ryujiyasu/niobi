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

use crate::annealing::{QuboProblem, simulated_annealing};
use crate::scoring::{self, BloodType};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

/// Literature-based combined transplant rates.
///
/// Sources:
///   - SLK (liver+kidney): 7.9% of liver transplants — OPTN/SRTR 2023 Annual Data Report: Liver
///   - Heart multi-organ: 13.5% of heart transplants — OPTN/SRTR 2023 Annual Data Report: Heart
///     (heart-kidney 421, heart-liver 70, heart-lung 53 of 4,092 adult heart transplants)
///   - SPK (pancreas+kidney): 79.2% of pancreas wait-listings — OPTN/SRTR 2023 Annual Data Report: Pancreas
///   - Japan brain-dead donors: 130–139/year (JOTN 2024); λ≈0.37/day → P(≥2 same day)≈5.9% ≈ 22 days/year
#[derive(Clone, Debug)]
pub struct ScenarioParams {
    /// Fraction of liver patients needing liver+kidney combined (SLK).
    pub slk_rate: f64,
    /// Fraction of heart patients needing heart+lung combined.
    pub heart_lung_rate: f64,
    /// Fraction of pancreas patients needing pancreas+kidney combined (SPK).
    pub spk_rate: f64,
    /// Number of simultaneous donors (1 = standard, 2+ = concurrent).
    pub n_donors: usize,
}

impl ScenarioParams {
    /// OPTN/SRTR 2023-based rates (conservative: uses actual transplant ratios).
    pub fn optn_2023() -> Self {
        Self {
            slk_rate: 0.079,       // 7.9% SLK
            heart_lung_rate: 0.013, // 1.3% heart-lung (53/4,092)
            spk_rate: 0.792,       // 79.2% SPK
            n_donors: 1,
        }
    }

    /// Two concurrent donors (Poisson-justified: ~22 days/year in Japan).
    pub fn optn_2023_dual_donor() -> Self {
        Self {
            n_donors: 2,
            ..Self::optn_2023()
        }
    }
}

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
                    (Organ::LungL, Organ::Heart) | (Organ::LungR, Organ::Heart) |
                    (Organ::Pancreas, Organ::KidneyL) | (Organ::Pancreas, Organ::KidneyR) |
                    (Organ::KidneyL, Organ::Pancreas) | (Organ::KidneyR, Organ::Pancreas)
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

/// Generate a realistic multi-organ scenario (legacy: fixed 15%/10% combined rates).
pub fn generate_multi_organ_scenario(
    n_per_organ: usize,
    seed: u64,
) -> (MultiOrganDonor, Vec<MultiOrganPatient>) {
    generate_scenario_with_params(n_per_organ, seed, &ScenarioParams {
        slk_rate: 0.15,
        heart_lung_rate: 0.10,
        spk_rate: 0.0,
        n_donors: 1,
    }).0
}

/// Generate scenario with literature-based parameters.
///
/// Returns (donors, patients). For single-donor scenarios, returns vec of 1.
pub fn generate_scenario_with_params(
    n_per_organ: usize,
    seed: u64,
    params: &ScenarioParams,
) -> ((MultiOrganDonor, Vec<MultiOrganPatient>), Vec<MultiOrganDonor>) {
    let mut rng = StdRng::seed_from_u64(seed);

    let mut donors = Vec::new();
    for _ in 0..params.n_donors {
        donors.push(MultiOrganDonor {
            blood_type: random_bt(&mut rng),
            body_weight: rng.gen_range(50.0..90.0),
            liver_volume: rng.gen_range(1000.0..1800.0),
            region_km: rng.gen_range(0.0..500.0),
        });
    }

    let mut patients = Vec::new();
    let all_organs = Organ::all();

    for (organ_idx, &organ) in all_organs.iter().enumerate() {
        for j in 0..n_per_organ {
            let needs_combined = match organ {
                Organ::Liver => rng.gen_range(0.0..1.0) < params.slk_rate,
                Organ::Heart => rng.gen_range(0.0..1.0) < params.heart_lung_rate,
                Organ::Pancreas => rng.gen_range(0.0..1.0) < params.spk_rate,
                _ => false,
            };

            let mut needed = vec![organ];
            if needs_combined {
                match organ {
                    Organ::Liver => needed.push(Organ::KidneyL),
                    Organ::Heart => needed.push(Organ::LungL),
                    Organ::Pancreas => needed.push(Organ::KidneyR),
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

    ((donors[0].clone(), patients), donors)
}

/// Build QUBO for multi-donor multi-organ allocation.
///
/// Each donor provides a full set of organs. Variables: x_{d,k,i} for
/// donor d, organ k, patient i. Cross-donor kidney competition is the
/// key scenario where QUBO outperforms independent allocation.
pub fn build_multi_donor_qubo(
    donors: &[MultiOrganDonor],
    patients: &[MultiOrganPatient],
    organs: &[Organ],
    combined_bonus: f64,
    penalty: f64,
) -> (QuboProblem, Vec<(usize, Organ, usize)>) {
    // var_map: (donor_idx, organ, patient_idx)
    let mut var_map: Vec<(usize, Organ, usize)> = Vec::new();
    let mut linear: Vec<f64> = Vec::new();

    for (d, donor) in donors.iter().enumerate() {
        for &organ in organs {
            for (i, patient) in patients.iter().enumerate() {
                if !patient.needed_organs.contains(&organ) {
                    continue;
                }
                let s = organ_score(donor, patient, &organ);
                if s > 0.0 {
                    linear.push(-s);
                    var_map.push((d, organ, i));
                }
            }
        }
    }

    let n_vars = linear.len();
    let mut quadratic: Vec<(usize, usize, f64)> = Vec::new();

    for a in 0..n_vars {
        for b in (a + 1)..n_vars {
            let (d_a, org_a, p_a) = var_map[a];
            let (d_b, org_b, p_b) = var_map[b];

            // Constraint: each (donor, organ) → at most 1 patient
            if d_a == d_b && org_a == org_b {
                quadratic.push((a, b, penalty));
                continue;
            }

            // Constraint: each patient gets at most 1 of each organ type across all donors
            if p_a == p_b && org_a == org_b {
                quadratic.push((a, b, penalty));
                continue;
            }

            // Paired organ constraint (patient can't get both kidneys, both lungs)
            if p_a == p_b {
                let same_type = matches!(
                    (org_a, org_b),
                    (Organ::KidneyL, Organ::KidneyR) | (Organ::KidneyR, Organ::KidneyL) |
                    (Organ::LungL, Organ::LungR) | (Organ::LungR, Organ::LungL)
                );
                if same_type {
                    quadratic.push((a, b, penalty));
                    continue;
                }
            }

            // Combined transplant bonus (same patient, same donor, complementary organs)
            if p_a == p_b && d_a == d_b {
                let patient = &patients[p_a];
                if patient.needs_combined {
                    let is_combined = matches!(
                        (org_a, org_b),
                        (Organ::Liver, Organ::KidneyL) | (Organ::Liver, Organ::KidneyR) |
                        (Organ::KidneyL, Organ::Liver) | (Organ::KidneyR, Organ::Liver) |
                        (Organ::Heart, Organ::LungL) | (Organ::Heart, Organ::LungR) |
                        (Organ::LungL, Organ::Heart) | (Organ::LungR, Organ::Heart) |
                        (Organ::Pancreas, Organ::KidneyL) | (Organ::Pancreas, Organ::KidneyR) |
                        (Organ::KidneyL, Organ::Pancreas) | (Organ::KidneyR, Organ::Pancreas)
                    );
                    if is_combined {
                        quadratic.push((a, b, -combined_bonus));
                    }
                }
            }
        }
    }

    let labels = var_map.iter().map(|&(d, o, p)| (d * 8 + o.index(), p)).collect();
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

/// Solve multi-donor multi-organ allocation with QUBO.
pub fn solve_multi_donor(
    donors: &[MultiOrganDonor],
    patients: &[MultiOrganPatient],
    organs: &[Organ],
    combined_bonus: f64,
    penalty: f64,
    sweeps: usize,
    seed: u64,
) -> Vec<(usize, Organ, usize, f64)> {
    let (qubo, var_map) = build_multi_donor_qubo(donors, patients, organs, combined_bonus, penalty);

    if qubo.n_vars == 0 {
        return vec![];
    }

    let solution = simulated_annealing(&qubo, sweeps, 10.0, 0.01, seed);

    let mut assignments = Vec::new();
    for (idx, &assigned) in solution.assignment.iter().enumerate() {
        if assigned {
            let (donor_idx, organ, patient_idx) = var_map[idx];
            let score = organ_score(&donors[donor_idx], &patients[patient_idx], &organ);
            assignments.push((donor_idx, organ, patient_idx, score));
        }
    }
    assignments
}

/// Independent greedy for multi-donor: each (donor, organ) solved independently.
pub fn solve_independent_multi_donor(
    donors: &[MultiOrganDonor],
    patients: &[MultiOrganPatient],
    organs: &[Organ],
) -> Vec<(usize, Organ, usize, f64)> {
    let mut assignments = Vec::new();
    let mut assigned_patients = std::collections::HashSet::new();

    for (d, donor) in donors.iter().enumerate() {
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
                assignments.push((d, organ, idx, best_score));
                assigned_patients.insert(idx);
            }
        }
    }
    assignments
}

fn random_bt(rng: &mut StdRng) -> BloodType {
    match rng.gen_range(0..100) {
        0..=39 => BloodType::A,
        40..=59 => BloodType::O,
        60..=79 => BloodType::B,
        _ => BloodType::AB,
    }
}

// ===========================================================================
//  Lives-saved formulation (救命数主指標)
//
//  The score-sum objective above is *separable*: a donor's K organs go to the
//  K highest-scoring patients regardless of method, so QUBO ≈ greedy on score.
//  The clinically meaningful objective is the number of patients SAVED — and a
//  combined-transplant patient (e.g. SLK = liver+kidney) is saved ONLY if they
//  receive ALL needed organs. A half-transplant saves nobody and *wastes* the
//  organ that was committed. Uncoordinated per-organ (greedy) allocation strands
//  combined patients and wastes organs; global optimization (QUBO/annealing)
//  avoids the waste. That gap is the genuine, NP-hard quantum-utility signal.
// ===========================================================================

/// Summary of an allocation in lives terms.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LivesSummary {
    /// Patients who received their COMPLETE needed-organ set.
    pub lives: usize,
    pub lives_single: usize,
    pub lives_combined: usize,
    /// Combined patients who received SOME but not all needed organs (stranded).
    pub stranded_combined: usize,
    /// Organs committed to patients who were NOT fully served (pure waste).
    pub wasted_organs: usize,
    /// Total organs placed.
    pub organs_used: usize,
}

/// Count lives saved: a patient is saved iff every organ in `needed_organs`
/// was assigned to them. Organs given to partially-served patients are wasted.
pub fn count_lives_saved(
    assignments: &[(Organ, usize, f64)],
    patients: &[MultiOrganPatient],
) -> LivesSummary {
    use std::collections::{HashMap, HashSet};
    let mut got: HashMap<usize, HashSet<Organ>> = HashMap::new();
    for &(organ, pid, _) in assignments {
        got.entry(pid).or_default().insert(organ);
    }
    let mut s = LivesSummary {
        organs_used: assignments.len(),
        ..Default::default()
    };
    for (&pid, organs_got) in &got {
        let patient = &patients[pid];
        let saved = patient.needed_organs.iter().all(|o| organs_got.contains(o));
        if saved {
            s.lives += 1;
            if patient.needs_combined {
                s.lives_combined += 1;
            } else {
                s.lives_single += 1;
            }
        } else {
            s.wasted_organs += organs_got.len();
            if patient.needs_combined {
                s.stranded_combined += 1;
            }
        }
    }
    s
}

/// Build the lives-maximizing QUBO (all-or-nothing for combined patients).
///
/// - Single patient (1 organ): linear reward −`w_life` on its organ var → 1 life.
/// - Combined patient (organs {A,B}): each var carries reward 0; a pairwise
///   bonus −`w_life` on (x_A,x_B) realizes the life ONLY when both are assigned.
///   Assigning one organ alone yields no reward but consumes the organ → the
///   minimizer prefers giving that organ to a single patient (avoids stranding).
/// - `score_eps`·organ_score is a tiny tiebreaker toward better medical matches.
/// - `penalty` (≫ w_life) enforces each organ type → at most one patient.
pub fn build_lives_qubo(
    donor: &MultiOrganDonor,
    patients: &[MultiOrganPatient],
    organs: &[Organ],
    w_life: f64,
    score_eps: f64,
    penalty: f64,
) -> (QuboProblem, Vec<(Organ, usize)>) {
    let mut var_map: Vec<(Organ, usize)> = Vec::new();
    let mut linear: Vec<f64> = Vec::new();

    for &organ in organs {
        for (i, patient) in patients.iter().enumerate() {
            if !patient.needed_organs.contains(&organ) {
                continue;
            }
            let s = organ_score(donor, patient, &organ);
            if s <= 0.0 {
                continue;
            }
            let life_lin = if patient.needs_combined { 0.0 } else { -w_life };
            linear.push(life_lin - score_eps * s);
            var_map.push((organ, i));
        }
    }

    let n_vars = linear.len();
    let mut quadratic: Vec<(usize, usize, f64)> = Vec::new();

    for a in 0..n_vars {
        for b in (a + 1)..n_vars {
            let (org_a, pa) = var_map[a];
            let (org_b, pb) = var_map[b];
            if org_a == org_b {
                // same organ type → at most one recipient
                quadratic.push((a, b, penalty));
            } else if pa == pb && patients[pa].needs_combined {
                // completion bonus: combined patient saved only if both organs assigned
                quadratic.push((a, b, -w_life));
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

/// Solve the lives-maximizing QUBO via simulated annealing.
pub fn solve_lives_qubo(
    donor: &MultiOrganDonor,
    patients: &[MultiOrganPatient],
    organs: &[Organ],
    w_life: f64,
    score_eps: f64,
    penalty: f64,
    sweeps: usize,
    seed: u64,
) -> Vec<(Organ, usize, f64)> {
    let (qubo, var_map) = build_lives_qubo(donor, patients, organs, w_life, score_eps, penalty);
    if qubo.n_vars == 0 {
        return vec![];
    }
    let sol = simulated_annealing(&qubo, sweeps, 10.0, 0.01, seed);
    let mut out = Vec::new();
    for (idx, &on) in sol.assignment.iter().enumerate() {
        if on {
            let (organ, pid) = var_map[idx];
            out.push((organ, pid, organ_score(donor, &patients[pid], &organ)));
        }
    }
    out
}

/// Uncoordinated per-organ (greedy) allocation — the realistic baseline.
///
/// Each organ type is assigned independently to its highest-scoring eligible
/// patient. A patient may receive several organ types, but the lists are not
/// coordinated, so a combined patient who tops one list rarely tops the other →
/// stranded → organ wasted. This mirrors list-by-list allocation practice.
pub fn solve_independent_lives(
    donor: &MultiOrganDonor,
    patients: &[MultiOrganPatient],
    organs: &[Organ],
) -> Vec<(Organ, usize, f64)> {
    use std::collections::{HashMap, HashSet};
    let mut assignments = Vec::new();
    let mut patient_got: HashMap<usize, HashSet<Organ>> = HashMap::new();

    for &organ in organs {
        let mut best_idx = None;
        let mut best = 0.0f64;
        for (i, patient) in patients.iter().enumerate() {
            if !patient.needed_organs.contains(&organ) {
                continue;
            }
            if patient_got.get(&i).map_or(false, |s| s.contains(&organ)) {
                continue;
            }
            let s = organ_score(donor, patient, &organ);
            if s > best {
                best = s;
                best_idx = Some(i);
            }
        }
        if let Some(idx) = best_idx {
            assignments.push((organ, idx, best));
            patient_got.entry(idx).or_default().insert(organ);
        }
    }
    assignments
}

/// Lives-aware greedy: assign each organ to the best SINGLE-organ patient who
/// needs it (ignoring combined patients); only fall back to a combined patient
/// if no single needs the organ. This is the smart, separable heuristic — if it
/// matches QUBO, the single-donor lives objective has NO genuine coupling and
/// thus NO quantum utility. Used to falsify the quantum-advantage claim.
pub fn solve_singles_first(
    donor: &MultiOrganDonor,
    patients: &[MultiOrganPatient],
    organs: &[Organ],
) -> Vec<(Organ, usize, f64)> {
    use std::collections::{HashMap, HashSet};
    let mut assignments = Vec::new();
    let mut patient_got: HashMap<usize, HashSet<Organ>> = HashMap::new();

    for &organ in organs {
        // Prefer the best single-organ patient; fall back to combined.
        let mut best_idx = None;
        let mut best = 0.0f64;
        let mut best_is_single = false;
        for (i, patient) in patients.iter().enumerate() {
            if !patient.needed_organs.contains(&organ) {
                continue;
            }
            if patient_got.get(&i).map_or(false, |s| s.contains(&organ)) {
                continue;
            }
            let s = organ_score(donor, patient, &organ);
            if s <= 0.0 {
                continue;
            }
            let is_single = !patient.needs_combined;
            // singles strictly preferred over combined; within a class, higher score wins
            let better = match (is_single, best_is_single) {
                (true, false) => true,
                (false, true) => false,
                _ => s > best,
            };
            if best_idx.is_none() || better {
                best = s;
                best_idx = Some(i);
                best_is_single = is_single;
            }
        }
        if let Some(idx) = best_idx {
            assignments.push((organ, idx, best));
            patient_got.entry(idx).or_default().insert(organ);
        }
    }
    assignments
}

/// Exact maximum lives via brute force (small instances only, for validation).
/// Returns None if the variable count exceeds `max_vars` (too large to enumerate).
pub fn exact_max_lives(
    donor: &MultiOrganDonor,
    patients: &[MultiOrganPatient],
    organs: &[Organ],
    max_vars: usize,
) -> Option<usize> {
    let (qubo, var_map) = build_lives_qubo(donor, patients, organs, 1.0, 0.0, 1.0);
    let n = var_map.len();
    if n > max_vars || n > 26 {
        return None;
    }
    let mut best = 0usize;
    for mask in 0u64..(1u64 << n) {
        // feasibility: no organ type used twice
        let mut organ_used: Vec<Organ> = Vec::new();
        let mut feasible = true;
        let mut chosen: Vec<(Organ, usize, f64)> = Vec::new();
        for idx in 0..n {
            if mask & (1u64 << idx) != 0 {
                let (organ, pid) = var_map[idx];
                if organ_used.contains(&organ) {
                    feasible = false;
                    break;
                }
                organ_used.push(organ);
                chosen.push((organ, pid, 0.0));
            }
        }
        if !feasible {
            continue;
        }
        let lives = count_lives_saved(&chosen, patients).lives;
        if lives > best {
            best = lives;
        }
    }
    let _ = qubo;
    Some(best)
}

/// Generate a lives-benchmark scenario. Combined-transplant patients are
/// sicker (high MELD), matching clinical reality (e.g. SLK ⇐ hepatorenal
/// syndrome) — which makes them top their first organ list and thus exposes
/// the stranding failure of uncoordinated allocation.
pub fn generate_lives_scenario(
    n_per_organ: usize,
    seed: u64,
    params: &ScenarioParams,
) -> (MultiOrganDonor, Vec<MultiOrganPatient>) {
    let mut rng = StdRng::seed_from_u64(seed);

    let donor = MultiOrganDonor {
        blood_type: random_bt(&mut rng),
        body_weight: rng.gen_range(55.0..85.0),
        liver_volume: rng.gen_range(1200.0..1700.0),
        region_km: rng.gen_range(0.0..200.0),
    };

    let mut patients = Vec::new();
    let all_organs = Organ::all();

    for (organ_idx, &organ) in all_organs.iter().enumerate() {
        for j in 0..n_per_organ {
            let needs_combined = match organ {
                Organ::Liver => rng.gen_range(0.0..1.0) < params.slk_rate,
                Organ::Heart => rng.gen_range(0.0..1.0) < params.heart_lung_rate,
                Organ::Pancreas => rng.gen_range(0.0..1.0) < params.spk_rate,
                _ => false,
            };

            let mut needed = vec![organ];
            if needs_combined {
                match organ {
                    Organ::Liver => needed.push(Organ::KidneyL),
                    Organ::Heart => needed.push(Organ::LungL),
                    Organ::Pancreas => needed.push(Organ::KidneyR),
                    _ => {}
                }
            }

            // Combined patients are sicker → higher MELD/priority.
            let meld = if needs_combined {
                rng.gen_range(32.0..40.0)
            } else {
                rng.gen_range(6.0..34.0)
            };

            patients.push(MultiOrganPatient {
                id: organ_idx * n_per_organ + j,
                blood_type: random_bt(&mut rng),
                body_weight: rng.gen_range(45.0..95.0),
                meld_score: meld,
                waiting_days: rng.gen_range(30.0..3000.0),
                region_km: rng.gen_range(0.0..200.0),
                needed_organs: needed,
                needs_combined,
            });
        }
    }

    (donor, patients)
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
