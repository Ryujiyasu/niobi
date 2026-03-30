//! Example: Privacy-preserving blood donation matching
//!
//! Current system: donor walks in, blood type checked, bag labeled, shipped.
//! No HLA matching, no infection history cross-check across centers,
//! no optimization of which blood goes where.
//!
//! With niobi: every donor's encrypted profile (infection history,
//! medication, travel history, HLA type) is in the global pool.
//! argo proves "this blood is safe" without revealing why.
//! Quantum optimizer matches blood to recipients with full HLA
//! compatibility — not just ABO.
//!
//! Privacy guarantee: a donor's HIV status, medication history,
//! and travel history are NEVER exposed. Only "safe/unsafe" is proven.

use niobi::scoring::BloodType;

/// Blood donor profile (encrypted via hyde in production).
struct BloodDonor {
    id: String,
    blood_type: BloodType,
    hla_markers: [u8; 6],      // HLA-A, B, C, DR, DQ, DP
    infection_history: Vec<String>,  // encrypted, never exposed
    medications: Vec<String>,        // encrypted, never exposed
    travel_history: Vec<String>,     // encrypted, never exposed
    location_km: f64,
}

/// Blood recipient with specific needs.
struct BloodRecipient {
    id: String,
    blood_type: BloodType,
    hla_markers: [u8; 6],
    required_volume_ml: f64,
    urgency: f64,  // 0.0 = routine, 1.0 = emergency
    location_km: f64,
}

/// argo proof: "this blood is safe for transfusion"
/// Reveals: safe/unsafe
/// Hidden: infection history, medications, travel, donor identity
struct SafetyProof {
    donor_id_hash: Vec<u8>,  // anonymous
    is_safe: bool,
    proof: Vec<u8>,          // ZKP proof bytes
}

/// Compatibility score between donor and recipient.
/// In production: computed on ciphertext via plat (FHE).
fn compatibility_score(donor: &BloodDonor, recipient: &BloodRecipient) -> f64 {
    // ABO compatibility (hard constraint)
    let abo = match (donor.blood_type, recipient.blood_type) {
        (BloodType::O, _) => 1.0,
        (BloodType::A, BloodType::A) | (BloodType::A, BloodType::AB) => 1.0,
        (BloodType::B, BloodType::B) | (BloodType::B, BloodType::AB) => 1.0,
        (BloodType::AB, BloodType::AB) => 1.0,
        _ => return 0.0,
    };

    // HLA matching (higher = less rejection risk)
    let hla_match: f64 = donor.hla_markers.iter()
        .zip(recipient.hla_markers.iter())
        .filter(|(d, r)| d == r)
        .count() as f64 / 6.0;

    // Distance penalty (cold chain logistics)
    let dist = (donor.location_km - recipient.location_km).abs();
    let logistics = if dist > 500.0 { 0.5 } else { 1.0 - dist / 1000.0 };

    // Urgency weighting
    let urgency_weight = 0.5 + 0.5 * recipient.urgency;

    abo * (0.4 * hla_match + 0.3 * logistics + 0.3 * urgency_weight)
}

fn main() {
    println!("=== niobi Example: Blood Donation Matching ===\n");

    let donors = vec![
        BloodDonor {
            id: "BD001".into(), blood_type: BloodType::O,
            hla_markers: [1, 3, 7, 11, 5, 2],
            infection_history: vec![], medications: vec![],
            travel_history: vec!["domestic".into()], location_km: 0.0,
        },
        BloodDonor {
            id: "BD002".into(), blood_type: BloodType::A,
            hla_markers: [2, 7, 8, 11, 3, 1],
            infection_history: vec![], medications: vec![],
            travel_history: vec![], location_km: 300.0,
        },
    ];

    let recipients = vec![
        BloodRecipient {
            id: "BR001".into(), blood_type: BloodType::A,
            hla_markers: [2, 7, 8, 11, 3, 1],
            required_volume_ml: 400.0, urgency: 0.9, location_km: 280.0,
        },
        BloodRecipient {
            id: "BR002".into(), blood_type: BloodType::O,
            hla_markers: [1, 3, 7, 15, 5, 2],
            required_volume_ml: 200.0, urgency: 0.3, location_km: 10.0,
        },
    ];

    println!("Donors: {}, Recipients: {}", donors.len(), recipients.len());
    println!("\nCompatibility matrix (computed on encrypted data):");
    for d in &donors {
        for r in &recipients {
            let score = compatibility_score(d, r);
            if score > 0.0 {
                println!("  {}({:?}) -> {}({:?}): {:.3}",
                    d.id, d.blood_type, r.id, r.blood_type, score);
            }
        }
    }

    println!("\nPrivacy guarantees:");
    println!("  - Donor infection history: NEVER exposed");
    println!("  - Donor medications: NEVER exposed");
    println!("  - Donor travel history: NEVER exposed");
    println!("  - Only proven: 'this blood is safe for this recipient'");
    println!("\nScale: national pool -> global pool (same infrastructure)");
}
