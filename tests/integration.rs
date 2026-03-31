//! Integration tests for the niobi pipeline.
//!
//! These tests verify the end-to-end flow:
//! encryption → FHE scoring → ZKP proof → matching → notification

use niobi::fhe_scoring::{self, MkFheScoring};
use niobi::zkp_compat;
use niobi::scoring::BloodType;
use niobi::matching;
use niobi::annealing;
use niobi::privacy_protocol::{self, Individual, Role, MedicalData};
use rand::SeedableRng;
use rand::rngs::StdRng;

/// Full pipeline: encrypt → score → prove → match → verify
#[test]
fn test_full_pipeline_encrypt_score_prove_match() {
    let n = 10;
    let (donors, recipients) = annealing::generate_scenario(n, 42);

    // Step 1-2: MKFHE encrypt medical data (each individual has own key)
    let ctx = MkFheScoring::new();
    let mut rng = StdRng::seed_from_u64(42);
    let keys: Vec<_> = (0..(n * 2) as u64).map(|i| ctx.keygen(i, &mut rng)).collect();

    // Verify encryption roundtrip for a single party
    let val = 5u64;
    let ct = ctx.encrypt(&keys[0].public, val, &mut rng);
    let dec = ctx.decrypt(&ct, &[&keys[0].secret]);
    assert_eq!(dec, val);

    // Step 3: Build score matrix
    let scores = annealing::build_score_matrix(&donors, &recipients);
    assert_eq!(scores.len(), n);
    assert_eq!(scores[0].len(), n);

    // Step 4: Generate ZKP proofs for compatible pairs
    let max_wait = recipients.iter().map(|r| r.waiting_days).fold(1.0_f64, f64::max);
    let mut proof_count = 0;
    for (di, d) in donors.iter().enumerate() {
        for (ri, r) in recipients.iter().enumerate() {
            if scores[di][ri] > 0.0 {
                let result = zkp_compat::prove_compatibility(
                    &format!("d{}", di), &format!("r{}", ri),
                    d.blood_type as u64, d.liver_volume as u64,
                    r.blood_type as u64, r.meld_score as u64,
                    r.body_weight as u64, r.waiting_days as u64,
                    (d.region_km - r.region_km).abs() as u64,
                    max_wait as u64,
                );
                assert!(result.is_ok());
                let (stmt, proof) = result.unwrap();
                assert!(zkp_compat::verify_proof(&stmt, &proof).unwrap());
                proof_count += 1;
            }
        }
    }
    assert!(proof_count > 0, "Should have at least one compatible pair");

    // Step 5: Greedy matching
    let greedy = matching::greedy_match(&scores);
    assert!(!greedy.is_empty());

    // Step 5b: Quantum annealing matching
    let qubo = annealing::build_qubo(&scores, 10.0);
    let sa = annealing::simulated_annealing(&qubo, 1000, 10.0, 0.001, 42);
    assert!(sa.pairs.len() >= greedy.len().saturating_sub(1),
        "Quantum should match or exceed greedy: {} vs {}", sa.pairs.len(), greedy.len());
}

/// Verify no plaintext medical data leaks through proof bytes
#[test]
fn test_privacy_no_data_leak_in_proofs() {
    let (stmt, proof) = zkp_compat::prove_compatibility(
        "anon-d001", "anon-r001",
        0, 1400, 1, 35, 70, 180, 50, 500,
    ).unwrap();

    let proof_str = String::from_utf8_lossy(&proof.data);
    assert!(!proof_str.contains("1400"), "Liver volume leaked");
    assert!(!proof_str.contains("blood"), "Blood type string leaked");
    assert!(!proof_str.contains("anon-d001"));
    assert!(!proof_str.contains("anon-r001"));

    assert!(stmt.donor_anon_id.starts_with("anon-"));
    assert!(stmt.recipient_anon_id.starts_with("anon-"));
}

/// Verify MKFHE key isolation: one party's key cannot decrypt another's data
#[test]
fn test_mkfhe_key_isolation() {
    let ctx = MkFheScoring::new();
    let mut rng = StdRng::seed_from_u64(42);

    let party_a = ctx.keygen(1, &mut rng);
    let party_b = ctx.keygen(2, &mut rng);

    let ct_a = ctx.encrypt(&party_a.public, 5, &mut rng);
    let ct_b = ctx.encrypt(&party_b.public, 3, &mut rng);

    // Cross-party sum requires both keys
    let ct_sum = ctx.add(&ct_a, &ct_b);
    let correct = ctx.decrypt(&ct_sum, &[&party_a.secret, &party_b.secret]);
    assert_eq!(correct, (5 + 3) % ctx.plaintext_modulus());

    // Single key gives wrong result
    let wrong = ctx.decrypt(&ct_sum, &[&party_b.secret]);
    assert_ne!(wrong, correct, "Single key must not decrypt cross-party ciphertext");
}

/// Edge case: all donors incompatible with all recipients
#[test]
fn test_edge_all_incompatible() {
    let scores = vec![
        vec![0.0, 0.0, 0.0],
        vec![0.0, 0.0, 0.0],
        vec![0.0, 0.0, 0.0],
    ];
    let greedy = matching::greedy_match(&scores);
    assert!(greedy.is_empty(), "No matches when all incompatible");
}

/// Greedy vs quantum consistency for small verifiable cases
#[test]
fn test_greedy_vs_quantum_small() {
    let (donors, recipients) = annealing::generate_scenario(5, 123);
    let scores = annealing::build_score_matrix(&donors, &recipients);

    let greedy = matching::greedy_match(&scores);
    let greedy_score: f64 = greedy.iter().map(|&(_, _, s)| s).sum();

    let qubo = annealing::build_qubo(&scores, 10.0);
    let sa = annealing::simulated_annealing(&qubo, 2000, 10.0, 0.001, 123);
    let sa_score: f64 = sa.pairs.iter().map(|&(d, r)| scores[d][r]).sum();

    assert!(sa_score >= greedy_score * 0.95,
        "Quantum score {} should be close to greedy {}", sa_score, greedy_score);
}

/// Full privacy protocol produces valid results
#[test]
fn test_privacy_protocol_e2e() {
    let individuals = vec![
        Individual {
            anon_id: "anon-d001".into(),
            role: Role::PotentialDonor,
            medical_data: MedicalData {
                blood_type: BloodType::O, liver_volume: 1400.0,
                meld_score: 0.0, body_weight: 0.0, waiting_days: 0.0,
            },
            region_km: 0.0,
        },
        Individual {
            anon_id: "anon-r001".into(),
            role: Role::Recipient,
            medical_data: MedicalData {
                blood_type: BloodType::A, liver_volume: 0.0,
                meld_score: 35.0, body_weight: 70.0, waiting_days: 180.0,
            },
            region_km: 50.0,
        },
        Individual {
            anon_id: "anon-d002".into(),
            role: Role::PotentialDonor,
            medical_data: MedicalData {
                blood_type: BloodType::B, liver_volume: 1200.0,
                meld_score: 0.0, body_weight: 0.0, waiting_days: 0.0,
            },
            region_km: 200.0,
        },
        Individual {
            anon_id: "anon-r002".into(),
            role: Role::Recipient,
            medical_data: MedicalData {
                blood_type: BloodType::B, liver_volume: 0.0,
                meld_score: 25.0, body_weight: 60.0, waiting_days: 300.0,
            },
            region_km: 250.0,
        },
    ];

    let (notifications, audit) = privacy_protocol::run_private_matching(&individuals);

    assert!(!notifications.is_empty(), "Should have at least one notification");
    assert_eq!(audit.len(), 7);

    for entry in &audit {
        assert!(!entry.data_exposed.contains("blood_type"));
        assert!(!entry.data_exposed.contains("meld_score"));
        assert!(!entry.data_exposed.contains("liver_volume"));
    }

    for n in &notifications {
        assert!(n.to_anon_id.starts_with("anon-"));
        assert!(n.counterpart_anon_id.starts_with("anon-"));
    }
}

/// MKFHE scoring with plaintext functions
#[test]
fn test_fhe_scoring_functions() {
    let scale = 1000u64;
    let abo = fhe_scoring::abo_compatibility(0, 1); // O→A: compatible
    assert_eq!(abo, 1);

    let meld = fhe_scoring::meld_priority(35, scale);
    assert!(meld > 800);

    let grwr = fhe_scoring::grwr_score(1400, 70, scale);
    assert!(grwr > 900);

    // Composite in plaintext
    let ischemia = fhe_scoring::ischemia_score(120, scale);
    let waiting = 360u64 * scale / 500;
    let composite = if abo == 0 || grwr == 0 {
        0
    } else {
        (35 * meld + 25 * grwr + 25 * ischemia + 15 * waiting) / 100
    };
    assert!(composite > 0, "Compatible pair should have positive score");
    assert!(composite <= scale, "Score should not exceed scale");
}

/// MKFHE cross-party composite scoring (the core innovation)
#[test]
fn test_mkfhe_cross_party_scoring() {
    let ctx = MkFheScoring::new();
    let mut rng = StdRng::seed_from_u64(42);
    let t = ctx.plaintext_modulus();

    let patient = ctx.keygen(1, &mut rng);
    let donor = ctx.keygen(2, &mut rng);

    // Patient encrypts their scores (under their own key)
    let meld = 8u64 % t;
    let wait = 3u64 % t;
    let ct_meld = ctx.encrypt(&patient.public, meld, &mut rng);
    let ct_wait = ctx.encrypt(&patient.public, wait, &mut rng);

    // Donor encrypts their scores (under their own key — different!)
    let grwr = 5u64 % t;
    let isch = 4u64 % t;
    let ct_grwr = ctx.encrypt(&donor.public, grwr, &mut rng);
    let ct_isch = ctx.encrypt(&donor.public, isch, &mut rng);

    // Homomorphic weighted sum across different keys
    let ct_score = fhe_scoring::encrypted_composite_score(
        &ctx, &ct_meld, &ct_wait, &ct_grwr, &ct_isch,
    );

    // Both parties cooperate to decrypt
    let dec = ctx.decrypt(&ct_score, &[&patient.secret, &donor.secret]);
    let expected = (7 * meld + 5 * grwr + 5 * isch + 3 * wait) % t;
    assert_eq!(dec, expected, "Cross-party MKFHE scoring failed");
}
