//! Lives-saved benchmark: uncoordinated per-organ allocation (greedy) vs
//! global QUBO optimization (simulated annealing), measured in PATIENTS SAVED.
//!
//! A combined-transplant patient (e.g. SLK = liver+kidney) is saved only if
//! they receive ALL needed organs; a half-transplant wastes the committed
//! organ. Greedy strands combined patients and wastes organs; QUBO avoids the
//! waste. The gap is the genuine, NP-hard quantum-utility signal — and unlike
//! score-sum, it does NOT vanish at scale.

use niobi::multi_organ::*;
use std::time::Instant;

fn main() {
    println!("Lives-saved benchmark — greedy (uncoordinated per-organ) vs QUBO (simulated annealing)");
    println!("Combined patient saved ONLY if every needed organ received. OPTN/SRTR 2023 combined rates.\n");

    let params = ScenarioParams::optn_2023();
    let (w_life, eps, penalty) = (1.0, 0.01, 5.0);

    println!("== 3 organs (Liver, KidneyL, Heart) — with brute-force exact optimum ==");
    run(
        &[Organ::Liver, Organ::KidneyL, Organ::Heart],
        &[2, 3, 5],
        &params, w_life, eps, penalty, true, 3000,
    );

    println!("\n== 5 organs (Liver, KidneyL, KidneyR, Heart, Pancreas) ==");
    run(
        &[Organ::Liver, Organ::KidneyL, Organ::KidneyR, Organ::Heart, Organ::Pancreas],
        &[10, 30, 60, 100],
        &params, w_life, eps, penalty, false, 4000,
    );

    println!("\n== 8 organs (all) ==");
    run(
        Organ::all(),
        &[10, 50, 100],
        &params, w_life, eps, penalty, false, 4000,
    );
}

#[allow(clippy::too_many_arguments)]
fn run(
    organs: &[Organ],
    sizes: &[usize],
    params: &ScenarioParams,
    w_life: f64,
    eps: f64,
    penalty: f64,
    do_exact: bool,
    sweeps: usize,
) {
    println!(
        "{:>5} {:>8} {:>8} {:>9} {:>9} {:>9} {:>7} {:>6} {:>8}",
        "N/org", "patients", "combined", "greedyLv", "smartLv", "quboLv",
        "Q-smart", "exact", "qubo ms"
    );
    let seeds = [1u64, 2, 3, 4, 5];
    let k = seeds.len() as f64;
    for &n in sizes {
        let (mut gl, mut sl, mut ql, mut gw, mut qw) = (0.0, 0.0, 0.0, 0.0, 0.0);
        let (mut pat, mut comb, mut ms, mut ex) = (0.0, 0.0, 0.0, 0.0);
        let (mut g_done, mut g_str, mut q_done, mut q_str) = (0.0, 0.0, 0.0, 0.0);
        let mut exact_ok = true;
        for &seed in &seeds {
            let (donor, patients) = generate_lives_scenario(n, seed, params);
            let greedy = solve_independent_lives(&donor, &patients, organs);
            let t = Instant::now();
            let qubo = solve_lives_qubo(&donor, &patients, organs, w_life, eps, penalty, sweeps, seed);
            ms += t.elapsed().as_secs_f64() * 1000.0;

            let smart = solve_singles_first(&donor, &patients, organs);
            let g = count_lives_saved(&greedy, &patients);
            let sm = count_lives_saved(&smart, &patients);
            let q = count_lives_saved(&qubo, &patients);
            sl += sm.lives as f64;
            gl += g.lives as f64;
            ql += q.lives as f64;
            gw += g.wasted_organs as f64;
            qw += q.wasted_organs as f64;
            g_done += g.lives_combined as f64;
            g_str += g.stranded_combined as f64;
            q_done += q.lives_combined as f64;
            q_str += q.stranded_combined as f64;
            pat += patients.len() as f64;
            comb += patients.iter().filter(|p| p.needs_combined).count() as f64;

            if do_exact {
                match exact_max_lives(&donor, &patients, organs, 24) {
                    Some(e) => ex += e as f64,
                    None => exact_ok = false,
                }
            }
        }
        let exact_str = if do_exact && exact_ok {
            format!("{:.1}", ex / k)
        } else {
            "-".to_string()
        };
        println!(
            "{:>5} {:>8.0} {:>8.1} {:>9.1} {:>9.1} {:>9.1} {:>+7.1} {:>6} {:>8.1}",
            n, pat / k, comb / k, gl / k, sl / k, ql / k, (ql - sl) / k, exact_str, ms / k
        );
        println!(
            "        mechanism: greedy[done {:.1} strand {:.1}]  smartGreedy[lives {:.1}]  qubo[done {:.1} strand {:.1}]  Q-vs-smart={:+.1}",
            g_done / k, g_str / k, sl / k, q_done / k, q_str / k, (ql - sl) / k
        );
    }
}
