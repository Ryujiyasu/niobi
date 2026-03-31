//! Multi-organ QUBO benchmark: demonstrates NP-hard allocation
//! where Hungarian method cannot be applied.

use niobi::multi_organ::*;
use std::time::Instant;

fn main() {
    println!("Multi-organ QUBO allocation benchmark");
    println!("======================================\n");

    let organs_3 = &[Organ::Liver, Organ::KidneyL, Organ::Heart];
    let organs_5 = &[Organ::Liver, Organ::KidneyL, Organ::KidneyR, Organ::Heart, Organ::LungL];
    let organs_8 = Organ::all();

    println!("--- 3 organs (Liver, Kidney, Heart) ---\n");
    run_comparison(organs_3, &[5, 10, 20, 30, 50]);

    println!("--- 5 organs (+ KidneyR, LungL) ---\n");
    run_comparison(organs_5, &[5, 10, 20, 30]);

    println!("--- 8 organs (all) ---\n");
    run_comparison(organs_8, &[5, 10, 20]);
}

fn run_comparison(organs: &[Organ], sizes: &[usize]) {
    println!(
        "{:>5} {:>6} {:>8} {:>8} {:>8} {:>8} {:>10} {:>10}",
        "N/org", "total", "Indep#", "IndepS", "QUBO#", "QUBOS", "Indep ms", "QUBO ms"
    );

    for &n in sizes {
        let (donor, patients) = generate_multi_organ_scenario(n, 42);
        let total = patients.len();

        // Independent (greedy per organ)
        let t0 = Instant::now();
        let indep = solve_independent(&donor, &patients, organs);
        let indep_ms = t0.elapsed().as_secs_f64() * 1000.0;
        let indep_n = indep.len();
        let indep_s: f64 = indep.iter().map(|r| r.2).sum();

        // QUBO (simulated annealing)
        let sweeps = if n <= 20 { 100_000 } else { 50_000 };
        let t0 = Instant::now();
        let qubo = solve_multi_organ(&donor, &patients, organs, 3.0, 10.0, sweeps, 42);
        let qubo_ms = t0.elapsed().as_secs_f64() * 1000.0;
        let qubo_n = qubo.len();
        let qubo_s: f64 = qubo.iter().map(|r| r.2).sum();

        let improvement = if indep_s > 0.0 {
            (qubo_s - indep_s) / indep_s * 100.0
        } else {
            0.0
        };

        println!(
            "{:>5} {:>6} {:>8} {:>8.1} {:>8} {:>8.1} {:>10.1} {:>10.1}  ({:+.1}%)",
            n, total, indep_n, indep_s, qubo_n, qubo_s, indep_ms, qubo_ms, improvement
        );

        // Show combined transplants
        let combined_in_qubo: Vec<_> = qubo.iter()
            .filter(|(organ, pid, _)| {
                patients[*pid].needs_combined
            })
            .collect();
        if !combined_in_qubo.is_empty() {
            println!("        Combined transplants served by QUBO: {}", combined_in_qubo.len());
        }
    }
    println!();
}
