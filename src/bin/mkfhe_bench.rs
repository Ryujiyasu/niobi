//! MKFHE end-to-end benchmark: keygen, encrypt, score, decrypt.

use niobi::fhe_scoring::MkFheScoring;
use rand::rngs::StdRng;
use rand::SeedableRng;
use std::time::Instant;

fn main() {
    let ctx = MkFheScoring::new();
    let mut rng = StdRng::seed_from_u64(42);
    let t = ctx.plaintext_modulus();

    println!("plat MKFHE end-to-end benchmark");
    println!("================================");
    println!("params: N={}, q={}, t={}", ctx.params.n, ctx.params.q, t);
    println!();

    // --- Keygen ---
    let iterations = 100;
    let t0 = Instant::now();
    let mut keys = Vec::new();
    for i in 0..iterations {
        keys.push(ctx.keygen(i as u64, &mut rng));
    }
    let keygen_us = t0.elapsed().as_micros() as f64 / iterations as f64;
    println!("keygen:    {keygen_us:.1} µs/key ({iterations} iterations)");

    // --- Encrypt ---
    let kp_a = &keys[0];
    let kp_b = &keys[1];
    let t0 = Instant::now();
    let mut cts = Vec::new();
    for _ in 0..iterations {
        cts.push(ctx.encrypt(&kp_a.public, 7, &mut rng));
    }
    let encrypt_us = t0.elapsed().as_micros() as f64 / iterations as f64;
    println!("encrypt:   {encrypt_us:.1} µs/ct ({iterations} iterations)");

    // --- Scalar mul ---
    let ct = ctx.encrypt(&kp_a.public, 5, &mut rng);
    let t0 = Instant::now();
    for _ in 0..iterations {
        let _ = ctx.scalar_mul(&ct, 35);
    }
    let smul_us = t0.elapsed().as_micros() as f64 / iterations as f64;
    println!("scalar_mul: {smul_us:.1} µs/op ({iterations} iterations)");

    // --- Cross-party add ---
    let ct_a = ctx.encrypt(&kp_a.public, 3, &mut rng);
    let ct_b = ctx.encrypt(&kp_b.public, 7, &mut rng);
    let t0 = Instant::now();
    for _ in 0..iterations {
        let _ = ctx.add(&ct_a, &ct_b);
    }
    let add_us = t0.elapsed().as_micros() as f64 / iterations as f64;
    println!("cross-party add: {add_us:.1} µs/op ({iterations} iterations)");

    // --- Full composite score (2-party) ---
    let ct_meld = ctx.encrypt(&kp_a.public, 8, &mut rng);
    let ct_waiting = ctx.encrypt(&kp_a.public, 3, &mut rng);
    let ct_grwr = ctx.encrypt(&kp_b.public, 5, &mut rng);
    let ct_ischemia = ctx.encrypt(&kp_b.public, 4, &mut rng);

    let score_iters = 100;
    let t0 = Instant::now();
    for _ in 0..score_iters {
        let _ = niobi::fhe_scoring::encrypted_composite_score(
            &ctx, &ct_meld, &ct_waiting, &ct_grwr, &ct_ischemia,
        );
    }
    let score_us = t0.elapsed().as_micros() as f64 / score_iters as f64;
    println!("composite_score (2-party): {score_us:.1} µs/score ({score_iters} iterations)");

    // --- Cooperative decrypt (2-party) ---
    let ct_result = niobi::fhe_scoring::encrypted_composite_score(
        &ctx, &ct_meld, &ct_waiting, &ct_grwr, &ct_ischemia,
    );
    let sks = vec![&kp_a.secret, &kp_b.secret];
    let t0 = Instant::now();
    for _ in 0..iterations {
        let _ = ctx.decrypt(&ct_result, &sks);
    }
    let decrypt_us = t0.elapsed().as_micros() as f64 / iterations as f64;
    println!("cooperative_decrypt (2-party): {decrypt_us:.1} µs/op ({iterations} iterations)");

    // --- Full pipeline: keygen + encrypt 4 values + score + decrypt ---
    println!("\n--- Full pipeline (1 donor-patient pair) ---");
    let t0 = Instant::now();
    let pipeline_iters = 50;
    for i in 0..pipeline_iters {
        let patient = ctx.keygen(1000 + i, &mut rng);
        let donor = ctx.keygen(2000 + i, &mut rng);
        let ct_m = ctx.encrypt(&patient.public, 8, &mut rng);
        let ct_w = ctx.encrypt(&patient.public, 3, &mut rng);
        let ct_g = ctx.encrypt(&donor.public, 5, &mut rng);
        let ct_i = ctx.encrypt(&donor.public, 4, &mut rng);
        let ct_s = niobi::fhe_scoring::encrypted_composite_score(&ctx, &ct_m, &ct_w, &ct_g, &ct_i);
        let _ = ctx.decrypt(&ct_s, &[&patient.secret, &donor.secret]);
    }
    let pipeline_us = t0.elapsed().as_micros() as f64 / pipeline_iters as f64;
    let pipeline_ms = pipeline_us / 1000.0;
    println!("full pipeline: {pipeline_us:.0} µs ({pipeline_ms:.2} ms) per pair ({pipeline_iters} iterations)");

    // --- Scaling: 1×N scoring ---
    println!("\n--- 1×N scoring (1 donor vs N patients) ---");
    for n in [10, 50, 100, 200] {
        let donor = ctx.keygen(9999, &mut rng);
        let ct_g = ctx.encrypt(&donor.public, 5, &mut rng);
        let ct_i = ctx.encrypt(&donor.public, 4, &mut rng);

        let patients: Vec<_> = (0..n)
            .map(|j| {
                let kp = ctx.keygen(10000 + j, &mut rng);
                let ct_m = ctx.encrypt(&kp.public, (j % 10 + 1) as u64, &mut rng);
                let ct_w = ctx.encrypt(&kp.public, (j % 5 + 1) as u64, &mut rng);
                (kp, ct_m, ct_w)
            })
            .collect();

        let t0 = Instant::now();
        for (kp, ct_m, ct_w) in &patients {
            let ct_s = niobi::fhe_scoring::encrypted_composite_score(&ctx, ct_m, ct_w, &ct_g, &ct_i);
            let _ = ctx.decrypt(&ct_s, &[&kp.secret, &donor.secret]);
        }
        let total_ms = t0.elapsed().as_secs_f64() * 1000.0;
        let per_pair_ms = total_ms / n as f64;
        println!("N={n:>4}: {total_ms:.1}ms total, {per_pair_ms:.3}ms/pair");
    }
}
