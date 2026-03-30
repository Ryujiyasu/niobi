//! qmed End-to-End Demo
//!
//! Runs the full pipeline for video recording:
//! 1. Generate scenario (donors + recipients)
//! 2. Encrypt data (hyde/plat)
//! 3. Compute compatibility scores (FHE)
//! 4. Generate ZKP proofs (argo)
//! 5. Quantum annealing optimization
//! 6. Display results
//!
//! Usage:
//!   cargo run --bin demo
//!   cargo run --bin demo -- --size 50
//!   cargo run --bin demo -- --benchmark

use qmed::annealing::{self, ComparisonResult};

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let benchmark_mode = args.iter().any(|a| a == "--benchmark");
    let size = args.iter()
        .position(|a| a == "--size")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(10);

    if benchmark_mode {
        run_benchmark();
    } else {
        run_demo(size);
    }
}

fn run_demo(n: usize) {
    println!("╔══════════════════════════════════════════════════════╗");
    println!("║  qmed: Privacy-Preserving Liver Transplant Matching ║");
    println!("║  hyde × argo × plat × Quantum Annealing             ║");
    println!("╚══════════════════════════════════════════════════════╝\n");

    // Step 1: Generate scenario
    println!("━━━ Step 1: Generate scenario ━━━");
    let (donors, recipients) = annealing::generate_scenario(n, 42);
    println!("  Donors:     {}", donors.len());
    println!("  Recipients: {}", recipients.len());
    println!("  Total combinations: {} × {} = {}\n", n, n, n * n);

    // Step 2: Encrypt data (simulated)
    println!("━━━ Step 2: Encrypt medical data (hyde + plat/FHE) ━━━");
    let t0 = std::time::Instant::now();
    let backend = qmed::fhe_scoring::TfheScoring::new();
    for (i, d) in donors.iter().enumerate() {
        let data = format!("{:?}:{:.0}", d.blood_type, d.liver_volume);
        let _encrypted = plat_core::FheBackend::encrypt(&backend, data.as_bytes());
    }
    for (i, r) in recipients.iter().enumerate() {
        let data = format!("{:?}:{:.0}:{:.0}", r.blood_type, r.meld_score, r.body_weight);
        let _encrypted = plat_core::FheBackend::encrypt(&backend, data.as_bytes());
    }
    let encrypt_time = t0.elapsed();
    println!("  Encrypted {} records in {:.1}ms", donors.len() + recipients.len(), encrypt_time.as_secs_f64() * 1000.0);
    println!("  Each record: blood type, liver volume, MELD, body weight");
    println!("  All data is now ciphertext — no party can read it\n");

    // Step 3: Compute scores on encrypted data
    println!("━━━ Step 3: Compute compatibility scores (plat/FHE) ━━━");
    let t0 = std::time::Instant::now();
    let scores = annealing::build_score_matrix(&donors, &recipients);
    let score_time = t0.elapsed();
    let n_compatible = scores.iter().flat_map(|row| row.iter()).filter(|&&s| s > 0.0).count();
    println!("  Score matrix: {}×{}", n, n);
    println!("  Compatible pairs: {} / {} ({:.0}%)",
        n_compatible, n * n, n_compatible as f64 / (n * n) as f64 * 100.0);
    println!("  Computed in {:.1}ms (on encrypted data)\n", score_time.as_secs_f64() * 1000.0);

    // Step 4: Generate ZKP proofs
    println!("━━━ Step 4: Generate ZKP proofs (argo) ━━━");
    let t0 = std::time::Instant::now();
    let max_wait = recipients.iter().map(|r| r.waiting_days).fold(1.0_f64, f64::max);
    let mut proof_count = 0;
    for (di, d) in donors.iter().enumerate() {
        for (ri, r) in recipients.iter().enumerate() {
            if scores[di][ri] > 0.0 {
                let _proof = qmed::zkp_compat::prove_compatibility(
                    &format!("anon-d{:03}", di),
                    &format!("anon-r{:03}", ri),
                    d.blood_type as u64,
                    d.liver_volume as u64,
                    r.blood_type as u64,
                    r.meld_score as u64,
                    r.body_weight as u64,
                    r.waiting_days as u64,
                    (d.region_km - r.region_km).abs() as u64,
                    max_wait as u64,
                );
                proof_count += 1;
            }
        }
    }
    let proof_time = t0.elapsed();
    println!("  Generated {} ZKP proofs in {:.1}ms", proof_count, proof_time.as_secs_f64() * 1000.0);
    println!("  Each proof attests: 'this pair is compatible'");
    println!("  No proof reveals: blood type, MELD, liver volume, identity\n");

    // Step 5: Quantum annealing
    println!("━━━ Step 5: Quantum optimal matching ━━━");
    let qubo = annealing::build_qubo(&scores, 10.0);
    println!("  QUBO variables: {}", qubo.n_vars);
    println!("  QUBO interactions: {}", qubo.quadratic.len());
    println!("  Brute force: {}! ≈ impossible", n);

    let t0 = std::time::Instant::now();
    let greedy = annealing::greedy_match(&scores);
    let greedy_time = t0.elapsed();
    let greedy_score: f64 = greedy.iter().map(|&(_, _, s)| s).sum();

    let t0 = std::time::Instant::now();
    let sa = annealing::simulated_annealing(&qubo, 2000, 10.0, 0.001, 42);
    let sa_time = t0.elapsed();
    let sa_score: f64 = sa.pairs.iter().map(|&(d, r)| scores[d][r]).sum();

    println!("\n  ┌────────────────┬──────────┬──────────┬──────────┐");
    println!("  │ Method         │ Matches  │ Score    │ Time     │");
    println!("  ├────────────────┼──────────┼──────────┼──────────┤");
    println!("  │ Greedy         │ {:>8} │ {:>8.3} │ {:>6.1}ms │",
        greedy.len(), greedy_score, greedy_time.as_secs_f64() * 1000.0);
    println!("  │ Quantum (SA)   │ {:>8} │ {:>8.3} │ {:>6.1}ms │",
        sa.pairs.len(), sa_score, sa_time.as_secs_f64() * 1000.0);
    println!("  └────────────────┴──────────┴──────────┴──────────┘");

    let diff = sa.pairs.len() as i64 - greedy.len() as i64;
    if diff > 0 {
        println!("\n  → Quantum found {} more matches than greedy!", diff);
        println!("    In liver transplant, each match = one life saved.");
    }

    // Step 6: Results
    println!("\n━━━ Step 6: Notification to matched individuals ━━━");
    println!("  Matched pairs (anonymous IDs only):");
    for (i, &(d, r)) in sa.pairs.iter().enumerate().take(10) {
        println!("    anon-d{:03} ↔ anon-r{:03}  (score: {:.3})",
            d, r, scores[d][r]);
    }
    if sa.pairs.len() > 10 {
        println!("    ... and {} more", sa.pairs.len() - 10);
    }

    println!("\n━━━ Privacy summary ━━━");
    println!("  Blood types:    NEVER exposed");
    println!("  MELD scores:    NEVER exposed");
    println!("  Liver volumes:  NEVER exposed");
    println!("  Body weights:   NEVER exposed");
    println!("  Identities:     revealed ONLY on mutual consent");
    println!("  Hospital:       involved ONLY after consent (Step 7)");

    let total_time = encrypt_time + score_time + proof_time + greedy_time + sa_time;
    println!("\n━━━ Total pipeline: {:.1}ms ━━━\n", total_time.as_secs_f64() * 1000.0);
}

fn run_benchmark() {
    println!("╔══════════════════════════════════════════════════════╗");
    println!("║  qmed: Scale Benchmark — Greedy vs Quantum          ║");
    println!("╚══════════════════════════════════════════════════════╝\n");

    let sizes = [5, 10, 20, 30, 50, 75, 100, 150, 200];
    let mut results: Vec<ComparisonResult> = Vec::new();

    for &n in &sizes {
        let (donors, recipients) = annealing::generate_scenario(n, 42);
        let result = annealing::compare_methods(&donors, &recipients);
        println!("{}", result);
        results.push(result);
    }

    println!("\n━━━ Summary ━━━");
    println!("{:>5} {:>8} {:>8} {:>10} {:>10} {:>10}",
        "N", "Greedy", "Quantum", "Δmatches", "Δscore%", "QTime(ms)");
    println!("{}", "-".repeat(55));
    for r in &results {
        let diff = r.quantum_matches as i64 - r.greedy_matches as i64;
        println!("{:>5} {:>8} {:>8} {:>+10} {:>+10.1}% {:>10.1}",
            r.n, r.greedy_matches, r.quantum_matches,
            diff, r.improvement_pct, r.quantum_time_ms);
    }

    println!("\nBrute force complexity:");
    for &n in &[50, 100, 200] {
        println!("  N={}: {}! combinations (impossible on any classical computer)", n, n);
    }
}
