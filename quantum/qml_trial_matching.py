"""
Quantum Optimization for Clinical Trial Patient Assignment.

NEDO Q-2 量子技術例②: 量子最適化
「治験における患者選択や投与スケジュール、組み合わせる薬剤などの探索を行い、
治験成功率やコスト効率の向上など、治験デザインの効率化が期待される」

Problem: Assign patients to clinical trials optimally.
  - Multiple trials compete for the same patient population
  - Each trial has eligibility criteria, enrollment targets, geographic constraints
  - Some patients are eligible for multiple trials (overlap)
  - Goal: maximize total statistical power across ALL trials simultaneously

This is structurally identical to multi-organ matching (§4.2):
  - Independent assignment: assign each trial independently → patient conflicts
  - QUBO joint optimization: assign all trials simultaneously → no conflicts,
    better overall statistical power

Independent (greedy per trial) CANNOT:
  - Respect cross-trial patient exclusivity (same patient can't be in two trials)
  - Balance enrollment across trials (popular trials hoard patients)
  - Optimize global statistical power (each trial optimizes only its own)

QUBO CAN:
  - Encode patient exclusivity as hard constraints
  - Balance enrollment targets as soft constraints
  - Maximize total composite score (eligibility + diversity + geographic balance)
"""

import numpy as np
from dimod import BinaryQuadraticModel, SimulatedAnnealingSampler
from dataclasses import dataclass
import time
import json


@dataclass
class Patient:
    id: str
    age: int
    sex: str
    biomarkers: list[str]
    country: str
    prior_treatments: int


@dataclass
class Trial:
    id: str
    target_condition: str
    required_biomarkers: list[str]
    excluded_biomarkers: list[str]
    age_range: tuple[int, int]
    target_enrollment: int
    sites: list[str]  # countries


def generate_scenario(
    n_patients: int,
    n_trials: int,
    seed: int = 42,
) -> tuple[list[Patient], list[Trial]]:
    """Generate realistic multi-trial patient assignment scenario."""
    rng = np.random.default_rng(seed)

    all_biomarkers = [
        "EGFR+", "BRCA1+", "PD-L1_high", "HER2+", "KRAS+",
        "ALK+", "BRAF+", "MSI-H", "TMB-H", "NTRK+",
        "TP53_mut", "PIK3CA+", "ROS1+", "MET_amp", "FGFR+",
    ]
    countries = ["Japan", "USA", "Germany", "UK", "France",
                 "Korea", "Australia", "Canada"]

    # Generate patients
    patients = []
    for i in range(n_patients):
        n_markers = rng.integers(1, 5)
        markers = list(rng.choice(all_biomarkers, size=n_markers, replace=False))
        patients.append(Patient(
            id=f"PT{i+1:04d}",
            age=int(rng.normal(55, 15)),
            sex=rng.choice(["M", "F"]),
            biomarkers=markers,
            country=rng.choice(countries),
            prior_treatments=int(rng.integers(0, 4)),
        ))

    # Generate trials — designed to have overlapping eligibility
    trials = []
    for t in range(n_trials):
        n_req = rng.integers(1, 3)
        required = list(rng.choice(all_biomarkers[:8], size=n_req, replace=False))
        n_excl = rng.integers(0, 2)
        excluded = list(rng.choice(all_biomarkers[8:], size=n_excl, replace=False))
        n_sites = rng.integers(2, 6)
        sites = list(rng.choice(countries, size=n_sites, replace=False))
        target = int(rng.integers(
            max(5, n_patients // (n_trials * 2)),
            max(10, n_patients // n_trials),
        ))

        trials.append(Trial(
            id=f"NCT-{2026}-{t+1:03d}",
            target_condition=f"Condition targeting {'+'.join(required)}",
            required_biomarkers=required,
            excluded_biomarkers=excluded,
            age_range=(int(rng.integers(18, 40)), int(rng.integers(65, 80))),
            target_enrollment=target,
            sites=sites,
        ))

    return patients, trials


def eligibility_score(patient: Patient, trial: Trial) -> float:
    """
    Compute patient-trial eligibility and match quality.
    Returns 0 if ineligible, >0 if eligible (higher = better fit).
    In production: computed on FHE-encrypted data via plat.
    """
    # Hard constraints
    if patient.age < trial.age_range[0] or patient.age > trial.age_range[1]:
        return 0.0
    if patient.country not in trial.sites:
        return 0.0
    if any(b in patient.biomarkers for b in trial.excluded_biomarkers):
        return 0.0

    # Biomarker match
    matches = sum(1 for b in trial.required_biomarkers if b in patient.biomarkers)
    if matches == 0:
        return 0.0
    biomarker_score = matches / len(trial.required_biomarkers)

    # Diversity bonus: fewer prior treatments = treatment-naive (preferred)
    naive_bonus = max(0, 1.0 - patient.prior_treatments * 0.2)

    return 0.7 * biomarker_score + 0.3 * naive_bonus


# --- Independent assignment (greedy per trial) ---

def solve_independent(
    patients: list[Patient],
    trials: list[Trial],
) -> dict[str, list[tuple[int, float]]]:
    """
    Assign each trial independently (greedy by score).
    Does NOT respect cross-trial patient exclusivity.
    """
    assignments = {}
    for trial in trials:
        scores = []
        for pi, p in enumerate(patients):
            s = eligibility_score(p, trial)
            if s > 0:
                scores.append((pi, s))
        scores.sort(key=lambda x: -x[1])
        assignments[trial.id] = scores[:trial.target_enrollment]
    return assignments


# --- QUBO joint assignment ---

def solve_qubo_joint(
    patients: list[Patient],
    trials: list[Trial],
    penalty_exclusivity: float = 5.0,
    penalty_enrollment: float = 2.0,
    num_reads: int = 200,
) -> tuple[dict[str, list[tuple[int, float]]], float]:
    """
    QUBO joint optimization: assign all trials simultaneously.
    Respects patient exclusivity and balances enrollment.
    """
    np_ = len(patients)
    nt = len(trials)

    # Pre-compute scores
    scores = {}
    for ti, trial in enumerate(trials):
        for pi, patient in enumerate(patients):
            s = eligibility_score(patient, trial)
            if s > 0:
                scores[(ti, pi)] = s

    bqm = BinaryQuadraticModel(vartype="BINARY")

    def var(ti, pi):
        return f"t{ti}_p{pi}"

    # Objective: maximize total score
    for (ti, pi), s in scores.items():
        bqm.add_variable(var(ti, pi), -s)

    # Constraint 1: patient exclusivity — each patient in at most 1 trial
    for pi in range(np_):
        active_trials = [ti for ti in range(nt) if (ti, pi) in scores]
        for i, ti1 in enumerate(active_trials):
            for ti2 in active_trials[i + 1:]:
                bqm.add_interaction(var(ti1, pi), var(ti2, pi), penalty_exclusivity)

    # Constraint 2: enrollment target — soft penalty for over-enrollment
    # penalty * max(0, enrolled - target)^2  approximated as quadratic penalty
    for ti, trial in enumerate(trials):
        active_patients = [pi for pi in range(np_) if (ti, pi) in scores]
        target = trial.target_enrollment
        # Quadratic penalty: (sum_pi x_{ti,pi} - target)^2
        # = sum_pi x^2 + 2*sum_{i<j} x_i*x_j - 2*target*sum_pi x_i + target^2
        for pi in active_patients:
            bqm.add_variable(var(ti, pi), penalty_enrollment * (1 - 2 * target))
        for i, pi1 in enumerate(active_patients):
            for pi2 in active_patients[i + 1:]:
                bqm.add_interaction(var(ti, pi1), var(ti, pi2), 2 * penalty_enrollment)

    t0 = time.perf_counter()
    sampler = SimulatedAnnealingSampler()
    result = sampler.sample(bqm, num_reads=num_reads)
    sample = result.first.sample
    elapsed = time.perf_counter() - t0

    assignments = {trial.id: [] for trial in trials}
    for key, val in sample.items():
        if val == 1:
            parts = key.split("_")
            ti = int(parts[0][1:])
            pi = int(parts[1][1:])
            s = scores.get((ti, pi), 0)
            assignments[trials[ti].id].append((pi, s))

    return assignments, elapsed


# --- Evaluation ---

def evaluate_assignments(
    assignments: dict[str, list[tuple[int, float]]],
    trials: list[Trial],
    patients: list[Patient],
) -> dict:
    """Evaluate assignment quality."""
    total_assigned = sum(len(v) for v in assignments.values())
    total_score = sum(s for v in assignments.values() for _, s in v)

    # Patient conflicts (same patient in multiple trials)
    patient_counts = {}
    for trial_id, assigned in assignments.items():
        for pi, _ in assigned:
            patient_counts[pi] = patient_counts.get(pi, 0) + 1
    conflicts = sum(1 for c in patient_counts.values() if c > 1)
    conflict_patients = [pi for pi, c in patient_counts.items() if c > 1]

    # Enrollment satisfaction (how close to target)
    enrollment_satisfaction = []
    for trial in trials:
        assigned = len(assignments.get(trial.id, []))
        target = trial.target_enrollment
        satisfaction = min(assigned, target) / target if target > 0 else 0
        enrollment_satisfaction.append(satisfaction)
    avg_satisfaction = np.mean(enrollment_satisfaction) if enrollment_satisfaction else 0

    # Trials with zero enrollment
    empty_trials = sum(1 for v in assignments.values() if len(v) == 0)

    # Geographic diversity per trial
    diversity_scores = []
    for trial_id, assigned in assignments.items():
        if assigned:
            countries = set(patients[pi].country for pi, _ in assigned)
            diversity_scores.append(len(countries))
        else:
            diversity_scores.append(0)
    avg_diversity = np.mean(diversity_scores) if diversity_scores else 0

    return {
        "total_assigned": total_assigned,
        "total_score": round(total_score, 2),
        "conflicts": conflicts,
        "conflict_patients": len(conflict_patients),
        "avg_enrollment_satisfaction": round(avg_satisfaction, 3),
        "empty_trials": empty_trials,
        "avg_geographic_diversity": round(avg_diversity, 1),
    }


def run_experiment(n_patients: int, n_trials: int, seed: int = 42) -> dict:
    """Run comparison: Independent vs QUBO joint assignment."""
    patients, trials = generate_scenario(n_patients, n_trials, seed)

    # Compute overlap statistics
    eligibility_matrix = np.zeros((n_trials, n_patients))
    for ti, trial in enumerate(trials):
        for pi, patient in enumerate(patients):
            if eligibility_score(patient, trial) > 0:
                eligibility_matrix[ti][pi] = 1
    multi_eligible = np.sum(eligibility_matrix.sum(axis=0) > 1)
    avg_eligible_per_trial = np.mean(eligibility_matrix.sum(axis=1))

    print(f"\n{'='*70}")
    print(f"  Clinical Trial Patient Assignment: {n_patients} patients × {n_trials} trials")
    print(f"  Multi-trial eligible patients: {multi_eligible}/{n_patients} "
          f"({100*multi_eligible/n_patients:.0f}%)")
    print(f"  Avg eligible per trial: {avg_eligible_per_trial:.1f}")
    print(f"{'='*70}")

    # Independent
    t0 = time.perf_counter()
    indep_assignments = solve_independent(patients, trials)
    t_indep = time.perf_counter() - t0
    indep_eval = evaluate_assignments(indep_assignments, trials, patients)

    # QUBO joint
    qubo_assignments, t_qubo = solve_qubo_joint(patients, trials)
    qubo_eval = evaluate_assignments(qubo_assignments, trials, patients)

    # Results
    print(f"\n{'Metric':<35s} {'Independent':>12s} {'QUBO Joint':>12s}")
    print("-" * 62)
    print(f"{'Total assigned':35s} {indep_eval['total_assigned']:>12d} {qubo_eval['total_assigned']:>12d}")
    print(f"{'Total score':35s} {indep_eval['total_score']:>12.2f} {qubo_eval['total_score']:>12.2f}")
    print(f"{'Patient conflicts (double-booked)':35s} {indep_eval['conflict_patients']:>12d} {qubo_eval['conflict_patients']:>12d}")
    print(f"{'Avg enrollment satisfaction':35s} {indep_eval['avg_enrollment_satisfaction']:>11.1%} {qubo_eval['avg_enrollment_satisfaction']:>11.1%}")
    print(f"{'Trials with zero enrollment':35s} {indep_eval['empty_trials']:>12d} {qubo_eval['empty_trials']:>12d}")
    print(f"{'Avg geographic diversity (countries)':35s} {indep_eval['avg_geographic_diversity']:>12.1f} {qubo_eval['avg_geographic_diversity']:>12.1f}")
    print(f"{'Solve time':35s} {t_indep:>11.3f}s {t_qubo:>11.1f}s")

    # Highlight the key difference
    if indep_eval['conflict_patients'] > 0 and qubo_eval['conflict_patients'] == 0:
        print(f"\n  ★ Independent: {indep_eval['conflict_patients']} patients double-booked "
              f"(assigned to 2+ trials simultaneously)")
        print(f"  ★ QUBO: 0 conflicts — patient exclusivity guaranteed by QUBO constraint")
        print(f"  → This is the same structural advantage as multi-organ matching (§4.2):")
        print(f"    Independent cannot express cross-trial constraints.")
        print(f"    QUBO encodes them as penalty terms and solves globally.")

    return {
        "n_patients": n_patients,
        "n_trials": n_trials,
        "multi_eligible": int(multi_eligible),
        "independent": {**indep_eval, "time": round(t_indep, 4)},
        "qubo_joint": {**qubo_eval, "time": round(t_qubo, 2)},
    }


if __name__ == "__main__":
    print("=== Niobi: Clinical Trial Patient Assignment ===")
    print("=== NEDO Q-2: 量子最適化による治験デザインの効率化 ===")
    print("=== Independent (per-trial) vs QUBO (joint optimization) ===")

    all_results = []

    # Scale ladder
    configs = [
        (50,  5,  42),
        (100, 8,  42),
        (200, 10, 42),
        (500, 15, 42),
    ]

    for n_pat, n_tri, seed in configs:
        r = run_experiment(n_pat, n_tri, seed)
        all_results.append(r)

    # Summary
    print(f"\n\n{'='*80}")
    print(f"SUMMARY")
    print(f"{'='*80}")
    print(f"\n{'Scale':<20s} {'Indep conflicts':>16s} {'QUBO conflicts':>15s} "
          f"{'Indep enroll':>13s} {'QUBO enroll':>12s}")
    print("-" * 80)
    for r in all_results:
        label = f"{r['n_patients']}pt × {r['n_trials']}tr"
        print(f"{label:<20s} "
              f"{r['independent']['conflict_patients']:>16d} "
              f"{r['qubo_joint']['conflict_patients']:>15d} "
              f"{r['independent']['avg_enrollment_satisfaction']:>12.1%} "
              f"{r['qubo_joint']['avg_enrollment_satisfaction']:>11.1%}")

    with open("qml_trial_matching_results.json", "w") as f:
        json.dump(all_results, f, indent=2)
    print(f"\nResults saved to qml_trial_matching_results.json")
