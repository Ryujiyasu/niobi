//! Benchmark: Greedy vs Hungarian vs QUBO solver.
//! Directly addresses the concern that Greedy is too weak a baseline.

use niobi::annealing::{build_qubo, build_score_matrix, generate_scenario, simulated_annealing};
use niobi::matching::{greedy_match, hungarian_match};
use std::time::Instant;

fn main() {
    println!("Greedy vs Hungarian vs QUBO solver benchmark");
    println!("=============================================\n");
    println!(
        "{:>5} | {:>12} {:>10} {:>8} | {:>12} {:>10} {:>8} | {:>12} {:>10} {:>8} | {:>8} {:>8}",
        "N", "Greedy#", "Score", "ms",
        "Hungarian#", "Score", "ms",
        "QUBO SA#", "Score", "ms",
        "G gap%", "Q gap%"
    );
    println!("{}", "-".repeat(130));

    for n in [10, 20, 30, 50, 75, 100] {
        let (donors, recipients) = generate_scenario(n, 42);
        let scores = build_score_matrix(&donors, &recipients);

        // Greedy
        let t0 = Instant::now();
        let greedy = greedy_match(&scores);
        let greedy_ms = t0.elapsed().as_secs_f64() * 1000.0;
        let greedy_n = greedy.len();
        let greedy_score: f64 = greedy.iter().map(|m| m.2).sum();

        // Hungarian (optimal for single-organ bipartite matching)
        let t0 = Instant::now();
        let hungarian = hungarian_match(&scores);
        let hungarian_ms = t0.elapsed().as_secs_f64() * 1000.0;
        let hungarian_n = hungarian.len();
        let hungarian_score: f64 = hungarian.iter().map(|m| m.2).sum();

        // QUBO solver (simulated annealing)
        let qubo = build_qubo(&scores, 10.0);
        let t0 = Instant::now();
        let sweeps = if n <= 50 { 100_000 } else { 10_000 };
        let sa = simulated_annealing(&qubo, sweeps, 10.0, 0.01, 42);
        let qubo_ms = t0.elapsed().as_secs_f64() * 1000.0;
        let qubo_n = sa.pairs.len();
        let qubo_score: f64 = sa.pairs.iter().map(|&(d, r)| scores[d][r]).sum();

        let greedy_gap = if hungarian_score > 0.0 {
            (hungarian_score - greedy_score) / hungarian_score * 100.0
        } else {
            0.0
        };
        let qubo_gap = if hungarian_score > 0.0 {
            (hungarian_score - qubo_score) / hungarian_score * 100.0
        } else {
            0.0
        };

        println!(
            "{n:>5} | {greedy_n:>12} {greedy_score:>10.1} {greedy_ms:>8.3} | \
             {hungarian_n:>12} {hungarian_score:>10.1} {hungarian_ms:>8.3} | \
             {qubo_n:>12} {qubo_score:>10.1} {qubo_ms:>8.3} | \
             {greedy_gap:>7.1}% {qubo_gap:>7.1}%"
        );
    }

    println!("\nG gap% = Greedy's score gap from Hungarian optimal");
    println!("Q gap% = QUBO SA's score gap from Hungarian optimal");
    println!("Note: Hungarian gives the provably optimal solution for single-organ bipartite matching.");
    println!("QUBO solver's advantage is in multi-organ simultaneous allocation (NP-hard), not benchmarked here.");
}
