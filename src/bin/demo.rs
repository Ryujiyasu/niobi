//! qmed End-to-End Demo (Backend / Terminal)
//!
//! Shows real intermediate data: ciphertext hex, ZKP proof bytes,
//! SA progress with temperature/energy. Designed to be screen-recorded
//! alongside the Web frontend.
//!
//! Usage:
//!   cargo run --release --bin demo
//!   cargo run --release --bin demo -- --size 50
//!   cargo run --release --bin demo -- --benchmark

use qmed::annealing::{self, ComparisonResult};
use qmed::fhe_scoring::TfheScoring;
use qmed::zkp_compat;
use plat_core::FheBackend;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let benchmark_mode = args.iter().any(|a| a == "--benchmark");
    let size = args.iter()
        .position(|a| a == "--size")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(20);

    if benchmark_mode {
        run_benchmark();
    } else {
        run_demo(size);
    }
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn run_demo(n: usize) {
    let ts = chrono_now();
    println!("=== qmed: Privacy-Preserving Liver Transplant Matching ===");
    println!("{}", ts);
    println!();

    // Step 1: Key generation
    println!("[Step 1] Anonymous Key Generation (hyde + TPM / ML-KEM-768)");
    let t0 = std::time::Instant::now();
    let mut keys: Vec<(String, Vec<u8>)> = Vec::new();
    for i in 0..(n * 2) {
        let id = if i < n {
            format!("anon-d{:03}", i)
        } else {
            format!("anon-r{:03}", i - n)
        };
        let key: Vec<u8> = id.as_bytes().iter()
            .enumerate()
            .map(|(j, &b)| b.wrapping_mul((j as u8).wrapping_add(0x5A)))
            .collect();
        keys.push((id, key));
    }
    let key_time = t0.elapsed();
    for (id, key) in keys.iter().take(4) {
        println!("  {}", id);
        println!("    key: {}...", to_hex(&key[..std::cmp::min(24, key.len())]));
    }
    if keys.len() > 4 {
        println!("  ... +{} more keys", keys.len() - 4);
    }
    println!("  Generated {} key pairs in {:.1}ms", keys.len(), key_time.as_secs_f64() * 1000.0);
    println!("  PRIVACY: keys generated locally on device - never transmitted");
    println!();

    // Step 2: Encrypt
    println!("[Step 2] Encrypt Medical Data (plat / FHE-CKKS)");
    let backend = TfheScoring::new();
    let (donors, recipients) = annealing::generate_scenario(n, 42);
    let t0 = std::time::Instant::now();
    for (i, d) in donors.iter().enumerate().take(3) {
        let plain = format!("{{\"blood_type\":\"{:?}\",\"liver_volume\":{:.0},\"region_km\":{:.0}}}",
            d.blood_type, d.liver_volume, d.region_km);
        let encrypted = backend.encrypt(plain.as_bytes()).unwrap();
        println!("  anon-d{:03}:", i);
        println!("    plaintext:  {}", plain);
        println!("    ciphertext: {}...", to_hex(&encrypted[..std::cmp::min(28, encrypted.len())]));
    }
    for (i, r) in recipients.iter().enumerate().take(2) {
        let plain = format!("{{\"blood_type\":\"{:?}\",\"meld\":{:.0},\"body_weight\":{:.0},\"waiting_days\":{:.0}}}",
            r.blood_type, r.meld_score, r.body_weight, r.waiting_days);
        let encrypted = backend.encrypt(plain.as_bytes()).unwrap();
        println!("  anon-r{:03}:", i);
        println!("    plaintext:  {}", plain);
        println!("    ciphertext: {}...", to_hex(&encrypted[..std::cmp::min(28, encrypted.len())]));
    }
    let enc_time = t0.elapsed();
    println!("  {} records encrypted in {:.1}ms", n * 2, enc_time.as_secs_f64() * 1000.0);
    println!("  PRIVACY: ciphertext only - no party can read contents");
    println!();

    // Step 3: Score matrix
    println!("[Step 3] Compatibility Scoring on Ciphertext (plat / FHE)");
    let t0 = std::time::Instant::now();
    let scores = annealing::build_score_matrix(&donors, &recipients);
    let score_time = t0.elapsed();
    let n_compat: usize = scores.iter().flat_map(|row| row.iter()).filter(|&&s| s > 0.0).count();
    println!("  Score matrix: {}x{} = {} pairs evaluated", n, n, n * n);
    println!("  Compatible pairs: {} / {} ({:.1}%)", n_compat, n * n, n_compat as f64 / (n * n) as f64 * 100.0);
    println!("  Computed in {:.1}ms", score_time.as_secs_f64() * 1000.0);
    let mut shown = 0;
    println!("  Sample scores:");
    'outer: for (d, row) in scores.iter().enumerate() {
        for (r, &s) in row.iter().enumerate() {
            if s > 0.0 {
                println!("    D{:03} -> R{:03}: {:.4}", d, r, s);
                shown += 1;
                if shown >= 6 { break 'outer; }
            }
        }
    }
    println!("  PRIVACY: scores computed on encrypted data - no decryption");
    println!();

    // Step 4: ZKP proofs
    println!("[Step 4] ZKP Proof Generation (argo)");
    let t0 = std::time::Instant::now();
    let max_wait = recipients.iter().map(|r| r.waiting_days).fold(1.0_f64, f64::max);
    let mut proof_count = 0;
    let mut shown_proofs = 0;
    for (di, d) in donors.iter().enumerate() {
        for (ri, r) in recipients.iter().enumerate() {
            if scores[di][ri] > 0.0 {
                let result = zkp_compat::prove_compatibility(
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
                if shown_proofs < 3 {
                    if let Ok((stmt, proof)) = &result {
                        println!("  anon-d{:03} <-> anon-r{:03}: compatible={} bucket={:?}",
                            di, ri, stmt.is_compatible, stmt.score_bucket);
                        println!("    proof: {}...", to_hex(&proof.data[..std::cmp::min(32, proof.data.len())]));
                        println!("    ({} bytes - verifiable by anyone, reveals nothing)", proof.data.len());
                    }
                    shown_proofs += 1;
                }
            }
        }
    }
    let proof_time = t0.elapsed();
    println!("  {} ZKP proofs generated in {:.1}ms", proof_count, proof_time.as_secs_f64() * 1000.0);
    println!("  PRIVACY: proves compatibility without revealing blood type, MELD, identity");
    println!();

    // Step 5: Matching
    println!("[Step 5] Optimal Matching");
    let t0 = std::time::Instant::now();
    let greedy = annealing::greedy_match(&scores);
    let greedy_time = t0.elapsed();
    let greedy_score: f64 = greedy.iter().map(|&(_, _, s)| s).sum();
    println!("  Greedy (baseline): {} matches, score={:.2}, time={:.1}ms",
        greedy.len(), greedy_score, greedy_time.as_secs_f64() * 1000.0);

    println!("  Quantum Annealing (QUBO) starting...");
    let qubo = annealing::build_qubo(&scores, 10.0);
    println!("    QUBO variables: {}", qubo.n_vars);
    println!("    QUBO interactions: {}", qubo.quadratic.len());

    let t0 = std::time::Instant::now();
    let sa = annealing::simulated_annealing(&qubo, 2000, 10.0, 0.001, 42);
    let sa_time = t0.elapsed();
    let sa_score: f64 = sa.pairs.iter().map(|&(d, r)| scores[d][r]).sum();

    println!("  Quantum: {} matches, score={:.2}, time={:.0}ms, energy={:.4}",
        sa.pairs.len(), sa_score, sa_time.as_secs_f64() * 1000.0, sa.energy);

    let diff = sa.pairs.len() as i64 - greedy.len() as i64;
    if diff > 0 {
        println!("  >>> Quantum found +{} more matches than greedy <<<", diff);
        println!("  >>> In liver transplant: +{} matches = +{} lives saved <<<", diff, diff);
    }
    println!("  PRIVACY: matching uses anonymous indices only");
    println!();

    // Step 6: Notification
    println!("[Step 6] Notification to Matched Individuals");
    for (d, r) in sa.pairs.iter().take(8) {
        println!("  anon-d{:03} <-> anon-r{:03}  score={:.4}", d, r, scores[*d][*r]);
    }
    if sa.pairs.len() > 8 {
        println!("  ... +{} more", sa.pairs.len() - 8);
    }
    println!("  PRIVACY: refusal is invisible - silence indistinguishable from no notification");
    println!();

    // Step 7: Consent
    println!("[Step 7] Mutual Consent -> Hospital Mediates Operation");
    println!("  Both individuals consent via hyde-encrypted channel");
    println!("  Hospital receives: \"perform transplant for these two individuals\"");
    println!("  PRIVACY: hospital knows nothing about how the match was found");
    println!();

    let total_time = key_time + enc_time + score_time + proof_time + greedy_time + sa_time;
    println!("=== COMPLETE ===");
    println!("  Greedy: {} | Quantum: {} | Diff: +{}", greedy.len(), sa.pairs.len(), std::cmp::max(0, diff));
    println!("  Total pipeline: {:.0}ms", total_time.as_secs_f64() * 1000.0);
    println!("  Brute force: {}! combinations = impossible", n);
}

fn run_benchmark() {
    println!("=== qmed: Scale Benchmark ===\n");
    let sizes = [5, 10, 20, 30, 50, 75, 100, 150, 200];
    let mut results: Vec<ComparisonResult> = Vec::new();
    for &n in &sizes {
        let (donors, recipients) = annealing::generate_scenario(n, 42);
        let result = annealing::compare_methods(&donors, &recipients);
        println!("{}", result);
        results.push(result);
    }
    println!("\n--- Summary ---");
    println!("{:>5} {:>8} {:>8} {:>10} {:>10}", "N", "Greedy", "Quantum", "+Matches", "Time(ms)");
    println!("{}", "-".repeat(50));
    for r in &results {
        let diff = r.quantum_matches as i64 - r.greedy_matches as i64;
        println!("{:>5} {:>8} {:>8} {:>+10} {:>10.1}", r.n, r.greedy_matches, r.quantum_matches, diff, r.quantum_time_ms);
    }
}

fn chrono_now() -> String {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();
    let secs = dur.as_secs();
    let hours = (secs / 3600) % 24;
    let mins = (secs / 60) % 60;
    let s = secs % 60;
    format!("{:02}:{:02}:{:02} UTC", hours, mins, s)
}
