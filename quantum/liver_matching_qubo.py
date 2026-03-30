"""
Liver Donor Matching via Quantum Annealing (QUBO formulation).

Solves the optimal donor-recipient assignment problem for liver transplant
using D-Wave's simulated annealing. Compares with classical greedy approach.

QUBO formulation:
  - Binary variable x_{d,r} = 1 if donor d is matched to recipient r
  - Maximize total compatibility score
  - Constraints:
    * Each donor matched to at most one recipient
    * Each recipient matched to at most one donor
    * ABO-incompatible pairs forbidden
    * Graft-to-recipient weight ratio in safe range (0.8-1.2 GRWR)
"""

import numpy as np
from dimod import BinaryQuadraticModel, SimulatedAnnealingSampler
import time
import json
from itertools import permutations
from dataclasses import dataclass


@dataclass
class Donor:
    id: str
    hospital: str
    blood_type: str     # A, B, AB, O
    liver_volume: float  # mL (typical: 1000-1800)
    location_km: float


@dataclass
class Recipient:
    id: str
    hospital: str
    blood_type: str
    meld_score: float    # 6-40
    body_weight: float   # kg
    location_km: float
    waiting_days: float


# --- Compatibility scoring (mirrors Rust scoring.rs) ---

ABO_COMPAT = {
    ("O", "O"): 1, ("O", "A"): 1, ("O", "B"): 1, ("O", "AB"): 1,
    ("A", "A"): 1, ("A", "AB"): 1,
    ("B", "B"): 1, ("B", "AB"): 1,
    ("AB", "AB"): 1,
}


def abo_compatible(donor_bt: str, recip_bt: str) -> bool:
    return ABO_COMPAT.get((donor_bt, recip_bt), 0) == 1


def graft_to_recipient_weight_ratio(liver_volume_ml: float, body_weight_kg: float) -> float:
    """GRWR = graft volume / recipient body weight. Safe range: 0.8-1.2%."""
    return (liver_volume_ml / body_weight_kg) / 10  # convert to %


def grwr_score(grwr: float) -> float:
    """Score based on GRWR. Safe range: 0.8-5.0%, ideal ~2.0%."""
    if grwr < 0.8 or grwr > 5.0:
        return 0.0
    deviation = abs(grwr - 2.0)
    return max(0.0, 1.0 - deviation / 3.0)


def meld_priority(meld: float) -> float:
    return max(0.0, min(1.0, (meld - 6.0) / 34.0))


def ischemia_score(distance_km: float) -> float:
    hours = distance_km / 100.0
    if hours > 12.0:
        return 0.0
    return 1.0 - hours / 12.0


def waiting_priority(days: float, max_days: float) -> float:
    if max_days <= 0:
        return 0.0
    return min(1.0, days / max_days)


def composite_score(donor: Donor, recip: Recipient, max_wait: float) -> float:
    """Compute compatibility score. Returns 0 if hard constraints violated."""
    if not abo_compatible(donor.blood_type, recip.blood_type):
        return 0.0

    grwr = graft_to_recipient_weight_ratio(donor.liver_volume, recip.body_weight)
    gs = grwr_score(grwr)
    if gs == 0.0:
        return 0.0  # unsafe graft size

    ms = meld_priority(recip.meld_score)
    isch = ischemia_score(abs(donor.location_km - recip.location_km))
    wait = waiting_priority(recip.waiting_days, max_wait)

    # Weights: MELD urgency > graft fit > logistics > waiting time
    return 0.35 * ms + 0.25 * gs + 0.25 * isch + 0.15 * wait


# --- QUBO formulation ---

def build_qubo(donors: list[Donor], recipients: list[Recipient], penalty: float = 10.0) -> tuple:
    """
    Build QUBO for liver matching.

    Objective: maximize sum of scores * x_{d,r}
    Constraints (via penalty):
      - Each donor matched to at most 1 recipient
      - Each recipient matched to at most 1 donor
    """
    nd = len(donors)
    nr = len(recipients)
    max_wait = max((r.waiting_days for r in recipients), default=1.0)

    # Score matrix
    scores = np.zeros((nd, nr))
    for d in range(nd):
        for r in range(nr):
            scores[d][r] = composite_score(donors[d], recipients[r], max_wait)

    bqm = BinaryQuadraticModel(vartype="BINARY")

    # Variable naming: x_{d}_{r}
    def var(d, r):
        return f"x_{d}_{r}"

    # Linear terms: -score (minimization, so negate to maximize)
    for d in range(nd):
        for r in range(nr):
            if scores[d][r] > 0:
                bqm.add_variable(var(d, r), -scores[d][r])

    # Constraint: each donor matched to at most 1 recipient
    # penalty * (sum_r x_{d,r})^2 - penalty * sum_r x_{d,r}
    # = penalty * sum_{r1 != r2} x_{d,r1} * x_{d,r2}
    for d in range(nd):
        active_r = [r for r in range(nr) if scores[d][r] > 0]
        for i, r1 in enumerate(active_r):
            for r2 in active_r[i + 1:]:
                bqm.add_interaction(var(d, r1), var(d, r2), penalty)

    # Constraint: each recipient matched to at most 1 donor
    for r in range(nr):
        active_d = [d for d in range(nd) if scores[d][r] > 0]
        for i, d1 in enumerate(active_d):
            for d2 in active_d[i + 1:]:
                bqm.add_interaction(var(d1, r), var(d2, r), penalty)

    return bqm, scores


# --- Solvers ---

def solve_quantum(bqm, num_reads: int = 100) -> dict:
    """Solve QUBO using simulated annealing (D-Wave simulator)."""
    sampler = SimulatedAnnealingSampler()
    result = sampler.sample(bqm, num_reads=num_reads)
    return result.first.sample


def solve_greedy(scores: np.ndarray) -> list[tuple[int, int, float]]:
    """Greedy matching (same as Rust matching.rs)."""
    nd, nr = scores.shape
    candidates = []
    for d in range(nd):
        for r in range(nr):
            if scores[d][r] > 0:
                candidates.append((d, r, scores[d][r]))
    candidates.sort(key=lambda x: -x[2])

    matched_d = set()
    matched_r = set()
    result = []
    for d, r, s in candidates:
        if d not in matched_d and r not in matched_r:
            matched_d.add(d)
            matched_r.add(r)
            result.append((d, r, s))
    return result


def solve_bruteforce(scores: np.ndarray) -> list[tuple[int, int, float]]:
    """Exact optimal solution via brute force (only feasible for small N)."""
    nd, nr = scores.shape
    n = min(nd, nr)
    best_score = -1
    best_assignment = []

    for perm in permutations(range(nr), n):
        total = sum(scores[d][perm[d]] for d in range(n))
        if total > best_score:
            best_score = total
            best_assignment = [(d, perm[d], scores[d][perm[d]]) for d in range(n)
                               if scores[d][perm[d]] > 0]
    return best_assignment


def extract_matching(sample: dict, nd: int, nr: int) -> list[tuple[int, int]]:
    """Extract donor-recipient pairs from QUBO solution."""
    pairs = []
    for key, val in sample.items():
        if val == 1:
            parts = key.split("_")
            d, r = int(parts[1]), int(parts[2])
            pairs.append((d, r))
    return pairs


# --- Demo ---

def generate_scenario(n_donors: int, n_recipients: int, seed: int = 42) -> tuple:
    """Generate realistic liver transplant scenario."""
    rng = np.random.default_rng(seed)
    blood_types = ["O", "A", "B", "AB"]
    bt_weights = [0.30, 0.40, 0.20, 0.10]  # Japanese population distribution
    hospitals = ["Tokyo Medical", "Osaka University", "Kyushu University",
                 "Hokkaido University", "Tohoku University", "Nagoya University",
                 "Kyoto University", "Keio University"]
    locations = [0, 500, 900, 1200, 600, 350, 450, 50]  # km from Tokyo

    donors = []
    for i in range(n_donors):
        h_idx = rng.integers(0, len(hospitals))
        donors.append(Donor(
            id=f"D{i+1:03d}",
            hospital=hospitals[h_idx],
            blood_type=rng.choice(blood_types, p=bt_weights),
            liver_volume=rng.normal(1400, 200),  # mL
            location_km=locations[h_idx] + rng.normal(0, 30),
        ))

    recipients = []
    for i in range(n_recipients):
        h_idx = rng.integers(0, len(hospitals))
        recipients.append(Recipient(
            id=f"R{i+1:03d}",
            hospital=hospitals[h_idx],
            blood_type=rng.choice(blood_types, p=bt_weights),
            meld_score=rng.uniform(10, 40),
            body_weight=rng.normal(65, 15),
            location_km=locations[h_idx] + rng.normal(0, 30),
            waiting_days=rng.exponential(500),
        ))

    return donors, recipients


def run_comparison(n_donors: int, n_recipients: int, seed: int = 42):
    """Run and compare all solvers."""
    donors, recipients = generate_scenario(n_donors, n_recipients, seed)
    print(f"\n{'='*60}")
    print(f"Liver Transplant Matching: {n_donors} donors x {n_recipients} recipients")
    print(f"{'='*60}")

    # Build QUBO
    bqm, scores = build_qubo(donors, recipients)
    print(f"Variables: {len(bqm.variables)}, Interactions: {len(bqm.quadratic)}")

    # Greedy
    t0 = time.perf_counter()
    greedy_result = solve_greedy(scores)
    t_greedy = time.perf_counter() - t0
    greedy_score = sum(s for _, _, s in greedy_result)

    # Quantum (simulated annealing)
    t0 = time.perf_counter()
    qa_sample = solve_quantum(bqm)
    t_qa = time.perf_counter() - t0
    qa_pairs = extract_matching(qa_sample, n_donors, n_recipients)
    qa_score = sum(scores[d][r] for d, r in qa_pairs)

    # Brute force (only for small problems)
    if n_donors <= 7 and n_recipients <= 7:
        t0 = time.perf_counter()
        bf_result = solve_bruteforce(scores)
        t_bf = time.perf_counter() - t0
        bf_score = sum(s for _, _, s in bf_result)
    else:
        t_bf = float("inf")
        bf_score = None

    # Results
    print(f"\n{'Method':<25} {'Matches':>8} {'Score':>10} {'Time':>12}")
    print("-" * 58)
    print(f"{'Greedy':<25} {len(greedy_result):>8} {greedy_score:>10.4f} {t_greedy:>10.4f}s")
    print(f"{'Quantum (sim. anneal.)':<25} {len(qa_pairs):>8} {qa_score:>10.4f} {t_qa:>10.4f}s")
    if bf_score is not None:
        print(f"{'Brute Force (optimal)':<25} {len(bf_result):>8} {bf_score:>10.4f} {t_bf:>10.4f}s")
        gap_greedy = (bf_score - greedy_score) / bf_score * 100 if bf_score > 0 else 0
        gap_qa = (bf_score - qa_score) / bf_score * 100 if bf_score > 0 else 0
        print(f"\nOptimality gap:  Greedy {gap_greedy:.1f}%  |  Quantum {gap_qa:.1f}%")
    else:
        print(f"{'Brute Force':<25} {'N/A':>8} {'(too large)':>10} {'N/A':>12}")

    # Show matches
    print(f"\n--- Quantum Annealing Matches ---")
    for d, r in sorted(qa_pairs):
        don = donors[d]
        rec = recipients[r]
        grwr = graft_to_recipient_weight_ratio(don.liver_volume, rec.body_weight)
        print(f"  {don.id}({don.blood_type}, {don.hospital[:10]}) -> "
              f"{rec.id}({rec.blood_type}, MELD={rec.meld_score:.0f}, GRWR={grwr:.2f}%)")

    return {
        "n_donors": n_donors,
        "n_recipients": n_recipients,
        "greedy": {"matches": len(greedy_result), "score": greedy_score, "time": t_greedy},
        "quantum": {"matches": len(qa_pairs), "score": qa_score, "time": t_qa},
        "bruteforce": {"matches": len(bf_result) if bf_score else None,
                       "score": bf_score, "time": t_bf if bf_score else None},
    }


if __name__ == "__main__":
    print("=== qmed: Liver Transplant Matching - Quantum vs Classical ===")

    results = []
    # Scale up to see where brute force dies
    for n in [4, 6, 8, 12, 20]:
        r = run_comparison(n, n, seed=42)
        results.append(r)

    # Save results
    with open("matching_results.json", "w") as f:
        json.dump(results, f, indent=2, default=str)
    print(f"\nResults saved to matching_results.json")
