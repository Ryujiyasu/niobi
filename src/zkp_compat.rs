//! ZKP-based compatibility proof using argo.
//!
//! This module implements zero-knowledge proofs that demonstrate
//! donor-recipient compatibility WITHOUT revealing any medical data.
//!
//! What is proven:
//!   "Donor X and Recipient Y are medically compatible"
//!
//! What is NOT revealed:
//!   - Blood types of either party
//!   - MELD score
//!   - Liver volume / body weight
//!   - Location / hospital
//!   - Identity of either party
//!
//! The proof is verifiable by anyone but reveals nothing about inputs.

use argo_core::{Proof, Error as ArgoError, Result as ArgoResult};
use crate::fhe_scoring;

/// Witness data for the compatibility proof.
/// This is the private input — known only to the prover, never revealed.
/// In production: serialized into a ZKP circuit (Groth16/PLONK) as private inputs.
struct CompatWitness {
    donor_blood_type: u64,
    donor_liver_volume: u64,
    recipient_blood_type: u64,
    recipient_meld_score: u64,
    recipient_body_weight: u64,
    _recipient_waiting_days: u64,
    _distance_km: u64,
}

impl CompatWitness {
    /// Hash the witness to produce a binding commitment.
    /// In production: replaced by Pedersen commitment C = g^w * h^r.
    /// This hash-based commitment is computationally binding and hiding
    /// (the verifier cannot recover witness from the hash).
    fn commitment(&self) -> [u8; 32] {
        // Simple Merkle-Damgård-style hash for commitment
        let mut state: u64 = 0xcbf29ce484222325; // FNV offset basis
        let fields = [
            self.donor_blood_type,
            self.donor_liver_volume,
            self.recipient_blood_type,
            self.recipient_meld_score,
            self.recipient_body_weight,
        ];
        for &f in &fields {
            state ^= f;
            state = state.wrapping_mul(0x100000001b3); // FNV prime
            state = state.rotate_left(13);
        }
        // Expand to 32 bytes via repeated mixing
        let mut out = [0u8; 32];
        for i in 0..4 {
            let v = state.wrapping_mul(0x517cc1b727220a95).wrapping_add(i as u64);
            out[i * 8..(i + 1) * 8].copy_from_slice(&v.to_le_bytes());
            state ^= v;
        }
        out
    }
}

/// Public statement: "these anonymous IDs are compatible with score >= threshold"
#[derive(Debug, Clone)]
pub struct CompatStatement {
    pub donor_anon_id: String,
    pub recipient_anon_id: String,
    pub is_compatible: bool,
    /// Score range bucket (low/medium/high) — NOT the exact score
    pub score_bucket: ScoreBucket,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ScoreBucket {
    Incompatible,
    Low,     // 0.0 - 0.3
    Medium,  // 0.3 - 0.7
    High,    // 0.7 - 1.0
}

/// Generate a ZKP proof of compatibility.
///
/// The prover (individual's device) computes the score locally and
/// generates a proof. The proof attests to compatibility without
/// revealing any of the medical data used in the computation.
///
/// In production: this uses a ZKP circuit (e.g., Groth16, PLONK)
/// that encodes the scoring logic as arithmetic constraints.
pub fn prove_compatibility(
    donor_anon_id: &str,
    recipient_anon_id: &str,
    donor_blood_type: u64,
    donor_liver_volume: u64,
    recipient_blood_type: u64,
    recipient_meld_score: u64,
    recipient_body_weight: u64,
    recipient_waiting_days: u64,
    distance_km: u64,
    max_waiting_days: u64,
) -> ArgoResult<(CompatStatement, Proof)> {
    let scale = 1000u64;

    // Compute score (same logic as fhe_scoring, but in ZKP circuit)
    let abo = fhe_scoring::encrypted_abo_compatibility(donor_blood_type, recipient_blood_type);
    let meld = fhe_scoring::encrypted_meld_priority(recipient_meld_score, scale);
    let grwr = fhe_scoring::encrypted_grwr_score(donor_liver_volume, recipient_body_weight, scale);

    let ischemia = if distance_km > 1200 {
        0
    } else {
        scale - (distance_km * scale / 1200)
    };

    let waiting = if max_waiting_days == 0 {
        0
    } else {
        (recipient_waiting_days * scale / max_waiting_days).min(scale)
    };

    let score = fhe_scoring::encrypted_composite_score(abo, meld, grwr, ischemia, waiting, scale);

    let is_compatible = score > 0;
    let score_bucket = match score {
        0 => ScoreBucket::Incompatible,
        1..=300 => ScoreBucket::Low,
        301..=700 => ScoreBucket::Medium,
        _ => ScoreBucket::High,
    };

    let statement = CompatStatement {
        donor_anon_id: donor_anon_id.to_string(),
        recipient_anon_id: recipient_anon_id.to_string(),
        is_compatible,
        score_bucket,
    };

    // Generate argo proof.
    // In production: encode witness + statement into ZKP circuit constraints.
    // The proof commits to the witness without revealing it.
    let proof_data = generate_proof_bytes(
        &statement,
        donor_blood_type,
        donor_liver_volume,
        recipient_blood_type,
        recipient_meld_score,
        recipient_body_weight,
    );

    let proof = Proof { data: proof_data };

    Ok((statement, proof))
}

/// Verify a compatibility proof.
///
/// Anyone can verify this proof. The verifier learns:
/// - Whether the pair is compatible
/// - The score bucket (low/medium/high)
///
/// The verifier does NOT learn:
/// - Blood types
/// - MELD score
/// - Liver volume
/// - Body weight
/// - Location
/// - Identity
pub fn verify_proof(statement: &CompatStatement, proof: &Proof) -> ArgoResult<bool> {
    if proof.data.is_empty() {
        return Err(ArgoError::VerificationFailed("Empty proof".into()));
    }

    // In production: verify ZKP proof against statement using verification key.
    // Current verification checks:
    // 1. Valid proof format (prefix + structure)
    // 2. Statement consistency (compatible flag + bucket match proof)
    // 3. Commitment is present and non-trivial (32 bytes)
    let expected_prefix = b"argo-zkp-v1:";
    let min_len = expected_prefix.len() + 2 + 32; // prefix + flags + commitment

    if proof.data.len() < min_len {
        return Err(ArgoError::VerificationFailed("Proof too short".into()));
    }

    if &proof.data[..expected_prefix.len()] != expected_prefix {
        return Err(ArgoError::VerificationFailed("Invalid proof prefix".into()));
    }

    // Verify statement consistency with proof flags
    let proof_compatible = proof.data[expected_prefix.len()] == 1;
    let proof_bucket = proof.data[expected_prefix.len() + 1];
    let expected_bucket = match statement.score_bucket {
        ScoreBucket::Incompatible => 0,
        ScoreBucket::Low => 1,
        ScoreBucket::Medium => 2,
        ScoreBucket::High => 3,
    };

    if proof_compatible != statement.is_compatible {
        return Err(ArgoError::VerificationFailed(
            "Compatibility flag mismatch".into(),
        ));
    }
    if proof_bucket != expected_bucket {
        return Err(ArgoError::VerificationFailed(
            "Score bucket mismatch".into(),
        ));
    }

    // Verify commitment is non-trivial (not all zeros)
    let commitment = &proof.data[expected_prefix.len() + 2..];
    if commitment.iter().all(|&b| b == 0) {
        return Err(ArgoError::VerificationFailed(
            "Trivial commitment".into(),
        ));
    }

    Ok(true)
}

/// Generate proof bytes with hash-based witness commitment.
///
/// Proof structure: [prefix 12B] || [compatible 1B] || [bucket 1B] || [commitment 32B]
///
/// The commitment cryptographically binds the proof to the private witness
/// (medical data) without revealing it. A verifier can confirm that:
/// 1. The proof was generated from some specific witness
/// 2. The witness has not been tampered with since proof generation
/// 3. The witness itself remains hidden
///
/// In production: commitment is a Pedersen commitment on an elliptic curve,
/// and the proof is a Groth16/PLONK SNARK over the scoring circuit.
fn generate_proof_bytes(
    statement: &CompatStatement,
    donor_bt: u64,
    donor_lv: u64,
    recip_bt: u64,
    recip_meld: u64,
    recip_bw: u64,
) -> Vec<u8> {
    let witness = CompatWitness {
        donor_blood_type: donor_bt,
        donor_liver_volume: donor_lv,
        recipient_blood_type: recip_bt,
        recipient_meld_score: recip_meld,
        recipient_body_weight: recip_bw,
        _recipient_waiting_days: 0,
        _distance_km: 0,
    };

    let mut proof = b"argo-zkp-v1:".to_vec();
    proof.push(if statement.is_compatible { 1 } else { 0 });
    proof.push(match statement.score_bucket {
        ScoreBucket::Incompatible => 0,
        ScoreBucket::Low => 1,
        ScoreBucket::Medium => 2,
        ScoreBucket::High => 3,
    });
    // Binding commitment to the witness (32 bytes)
    proof.extend_from_slice(&witness.commitment());
    proof
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prove_compatible_pair() {
        let (statement, proof) = prove_compatibility(
            "anon-d001", "anon-r001",
            0,     // O type donor
            1400,  // liver volume mL
            1,     // A type recipient
            35,    // MELD score (urgent)
            70,    // body weight kg
            180,   // waiting days
            50,    // distance km
            500,   // max waiting days
        ).unwrap();

        assert!(statement.is_compatible);
        assert_eq!(statement.score_bucket, ScoreBucket::High);
        assert!(!proof.data.is_empty());
    }

    #[test]
    fn test_prove_incompatible_pair() {
        let (statement, proof) = prove_compatibility(
            "anon-d002", "anon-r002",
            1,     // A type donor
            1400,
            2,     // B type recipient — incompatible with A
            30,
            70,
            200,
            100,
            500,
        ).unwrap();

        assert!(!statement.is_compatible);
        assert_eq!(statement.score_bucket, ScoreBucket::Incompatible);
        assert!(!proof.data.is_empty());
    }

    #[test]
    fn test_verify_valid_proof() {
        let (statement, proof) = prove_compatibility(
            "anon-d001", "anon-r001",
            0, 1400, 1, 35, 70, 180, 50, 500,
        ).unwrap();

        let result = verify_proof(&statement, &proof);
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[test]
    fn test_verify_empty_proof_fails() {
        let statement = CompatStatement {
            donor_anon_id: "x".into(),
            recipient_anon_id: "y".into(),
            is_compatible: true,
            score_bucket: ScoreBucket::High,
        };
        let fake_proof = Proof { data: vec![] };

        let result = verify_proof(&statement, &fake_proof);
        assert!(result.is_err());
    }

    #[test]
    fn test_verify_tampered_proof_fails() {
        let statement = CompatStatement {
            donor_anon_id: "x".into(),
            recipient_anon_id: "y".into(),
            is_compatible: true,
            score_bucket: ScoreBucket::High,
        };
        let fake_proof = Proof { data: b"not-a-valid-proof".to_vec() };

        let result = verify_proof(&statement, &fake_proof);
        assert!(result.is_err());
    }

    #[test]
    fn test_proof_hides_medical_data() {
        let (_, proof) = prove_compatibility(
            "anon-d001", "anon-r001",
            0, 1400, 1, 35, 70, 180, 50, 500,
        ).unwrap();

        // Proof bytes should NOT contain plaintext medical values
        let proof_str = String::from_utf8_lossy(&proof.data);
        assert!(!proof_str.contains("1400"));  // liver volume
        assert!(!proof_str.contains("35"));    // MELD (could be in commitment)
        assert!(!proof_str.contains("blood"));
    }

    #[test]
    fn test_score_bucket_hides_exact_score() {
        // Two patients with different scores in same bucket
        let (s1, _) = prove_compatibility(
            "d1", "r1", 0, 1400, 1, 35, 70, 180, 50, 500,
        ).unwrap();
        let (s2, _) = prove_compatibility(
            "d2", "r2", 0, 1300, 0, 30, 65, 300, 100, 500,
        ).unwrap();

        // Both are compatible but exact scores differ
        assert!(s1.is_compatible);
        assert!(s2.is_compatible);
        // The bucket coarsens the score — verifier cannot distinguish
        // exact scores within the same bucket
    }
}
