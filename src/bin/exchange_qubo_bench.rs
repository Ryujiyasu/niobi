//! Exchange cycle-cover benchmark: greedy cycle-picking vs QUBO (simulated
//! annealing) vs brute-force exact, measured in TRANSPLANTS enabled.
//!
//! Maximum-weight vertex-disjoint cycle cover is genuinely NP-hard (weighted
//! set-packing). Greedy is provably suboptimal — an easy 2-way can block a
//! better 3-way sharing a pair. Unlike single-donor assignment (separable),
//! here global optimization should beat greedy and the gap should persist.

use niobi::exchange_chain::*;
use std::time::Instant;

fn main() {
    println!("Exchange cycle-cover benchmark — greedy vs QUBO (simulated annealing) vs exact");
    println!("Objective: maximize transplants via vertex-disjoint 2-/3-way exchange cycles.\n");

    let (life, eps, penalty) = (1.0, 0.05, 50.0);
    let seeds: Vec<u64> = (1..=8).collect();
    let k = seeds.len() as f64;

    println!(
        "{:>6} {:>9} {:>9} {:>9} {:>8} {:>9} {:>8}",
        "Npairs", "cycles", "greedyTx", "quboTx", "exactTx", "Q-greedy", "qubo ms"
    );
    // exact_cover auto-skips (returns None → "-") once cycles exceed this cap,
    // so large pools show the QUBO-vs-greedy gap where exact becomes intractable.
    let exact_cycle_cap = 220;
    for &n in &[8usize, 12, 16, 20, 30, 45, 60] {
        let (mut cyc, mut g, mut q, mut ex, mut ms) = (0.0, 0.0, 0.0, 0.0, 0.0);
        let mut exact_ok = true;
        let mut qwin = 0usize; // instances where QUBO strictly beat greedy
        for &seed in &seeds {
            let pairs = generate_exchange_pool(n, seed);
            let graph = build_compatibility_graph(&pairs);
            let ncyc = enumerate_cycles(&graph, 3).len();
            cyc += ncyc as f64;

            let gr = greedy_cover(&pairs, 3);
            let t = Instant::now();
            let qb = solve_cycle_cover_qubo(&pairs, 3, life, eps, penalty, 4000, seed);
            ms += t.elapsed().as_secs_f64() * 1000.0;

            g += gr.transplants as f64;
            q += qb.transplants as f64;
            if qb.transplants > gr.transplants {
                qwin += 1;
            }
            match exact_cover(&pairs, 3, exact_cycle_cap) {
                Some(e) => ex += e.transplants as f64,
                None => exact_ok = false,
            }
        }
        let exact_str = if exact_ok { format!("{:.1}", ex / k) } else { "-".to_string() };
        println!(
            "{:>6} {:>9.0} {:>9.1} {:>9.1} {:>8} {:>+9.1} {:>8.1}",
            n, cyc / k, g / k, q / k, exact_str, (q - g) / k, ms / k
        );
        println!("        QUBO strictly beat greedy in {}/{} instances", qwin, seeds.len());
    }
}
