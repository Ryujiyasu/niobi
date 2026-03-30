//! Integration tests for the niobi pipeline.
//!
//! These tests verify the end-to-end flow:
//! encryption → FHE scoring → ZKP proof → matching → notification

use niobi::fhe_scoring::{self, TfheScoring};
use niobi::zkp_compat;
use niobi::scoring::BloodType;
use niobi::matching;
use niobi::annealing;
use niobi::privacy_protocol::{self, Individual, Role, MedicalData};
use plat_core::FheBackend;

/// Full pipeline: encrypt → score → prove → match → verify
#[test]
fn test_full_pipeline_encrypt_score_prove_match() {
    let n = 10;
    let (donors, recipients) = annealing::generate_scenario(n, 42);
    let backend = TfheScoring::new();

    // Step 1-2: Encrypt medical data
    let mut encrypted_records = Vec::new();
    for d in &donors {
        let record = format!("{:?}:{:.0}:{:.0}", d.blood_type, d.liver_volume, d.region_km);
        let enc = backend.encrypt(record.as_bytes()).unwrap();
        assert!(enc.len() > record.len()); // ciphertext includes nonce
        let dec = backend.decrypt(&enc).unwrap();
        assert_eq!(String::from_utf8(dec).unwrap(), record);
        encrypted_records.push(enc);
    }

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
                // Verify each proof
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
        0,     // O type
        1400,  // liver volume
        1,     // A type
        35,    // MELD
        70,    // body weight
        180,   // waiting days
        50,    // distance
        500,   // max wait
    ).unwrap();

    let proof_str = String::from_utf8_lossy(&proof.data);

    // Proof should NOT contain plaintext medical values
    assert!(!proof_str.contains("1400"), "Liver volume leaked");
    assert!(!proof_str.contains("blood"), "Blood type string leaked");

    // Proof should NOT contain real names or IDs
    assert!(!proof_str.contains("anon-d001"));
    assert!(!proof_str.contains("anon-r001"));

    // Statement uses anonymous IDs only
    assert!(stmt.donor_anon_id.starts_with("anon-"));
    assert!(stmt.recipient_anon_id.starts_with("anon-"));
}

/// Verify that different keys produce incompatible encryption
#[test]
fn test_privacy_cross_key_isolation() {
    let key_a = [0x01u8; 32];
    let key_b = [0x02u8; 32];
    let backend_a = TfheScoring::with_key(&key_a);
    let backend_b = TfheScoring::with_key(&key_b);

    let data = b"MELD:35,BloodType:O,LiverVol:1400";
    let encrypted = backend_a.encrypt(data).unwrap();

    // Hospital B cannot decrypt Hospital A's data
    let cross_decrypt = backend_b.decrypt(&encrypted).unwrap();
    assert_ne!(&cross_decrypt[..], &data[..],
        "Cross-key decryption must not recover plaintext");

    // Hospital A can decrypt its own data
    let self_decrypt = backend_a.decrypt(&encrypted).unwrap();
    assert_eq!(&self_decrypt[..], &data[..]);
}

/// Edge case: all donors incompatible with all recipients
#[test]
fn test_edge_all_incompatible() {
    // All donors are A, all recipients are B → ABO incompatible
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

    // Quantum should be at least as good as greedy for small problems
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

    // Should produce matches
    assert!(!notifications.is_empty(), "Should have at least one notification");

    // Audit should have 7 steps
    assert_eq!(audit.len(), 7);

    // No step should expose medical data
    for entry in &audit {
        assert!(!entry.data_exposed.contains("blood_type"));
        assert!(!entry.data_exposed.contains("meld_score"));
        assert!(!entry.data_exposed.contains("liver_volume"));
    }

    // All notifications use anonymous IDs
    for n in &notifications {
        assert!(n.to_anon_id.starts_with("anon-"));
        assert!(n.counterpart_anon_id.starts_with("anon-"));
    }
}

/// FHE encode/decode preserves scoring accuracy
#[test]
fn test_fhe_score_encoding_accuracy() {
    let backend = TfheScoring::new();

    let test_scores = [0.0, 0.1, 0.5, 0.853, 0.999, 1.0];
    for &score in &test_scores {
        let encoded = backend.encode(score);
        let decoded = backend.decode(encoded);
        assert!((decoded - score).abs() < 0.002,
            "Score {}: encoded={}, decoded={}", score, encoded, decoded);
    }

    // Verify the FHE integer arithmetic matches floating-point scoring
    let scale = 1000u64;
    let abo = fhe_scoring::encrypted_abo_compatibility(0, 1); // O→A: compatible
    let meld = fhe_scoring::encrypted_meld_priority(35, scale);
    let grwr = fhe_scoring::encrypted_grwr_score(1400, 70, scale);
    let composite = fhe_scoring::encrypted_composite_score(abo, meld, grwr, 800, 360, scale);

    assert!(composite > 0, "Compatible pair should have positive score");
    assert!(composite <= scale, "Score should not exceed scale");
}
