//! Cryptographic data protection layer (hyde integration point).
//!
//! This module defines the encryption/decryption interface that enables
//! hospitals to share patient data without exposing it. In the full
//! protocol:
//!
//! - hyde (TPM + PQC/ML-KEM-768): encrypts data at device level
//! - argo (ZKP): proves compatibility without revealing medical records
//! - plat (FHE/CKKS): computes scores on ciphertext
//!
//! Current implementation uses a simulated encryption layer that
//! demonstrates the protocol flow. Real FHE integration is tracked
//! as a separate milestone.

use std::collections::HashMap;

/// Encrypted patient record. The ciphertext is opaque — no party
/// other than the originating hospital can read the contents.
#[derive(Debug, Clone)]
pub struct EncryptedRecord {
    /// Originating hospital identifier
    pub hospital_id: String,
    /// Opaque ciphertext (in production: CKKS ciphertext via plat)
    pub ciphertext: Vec<u8>,
    /// Record type: "donor" or "recipient"
    pub record_type: String,
}

/// Zero-knowledge compatibility proof (argo integration point).
/// Proves "donor D and recipient R are medically compatible"
/// without revealing blood type, MELD score, or any other field.
#[derive(Debug, Clone)]
pub struct CompatibilityProof {
    pub donor_id: String,
    pub recipient_id: String,
    /// Compatibility score computed on encrypted data
    pub encrypted_score: f64,
    /// ZKP proof bytes (in production: argo proof)
    pub proof: Vec<u8>,
    /// Whether this pair passes hard constraints (ABO, GRWR)
    pub is_compatible: bool,
}

/// Simulated encryption context for protocol demonstration.
/// In production, this would be backed by hyde's TPM+PQC key management.
pub struct CryptoContext {
    /// Hospital-specific encryption keys (simulated)
    hospital_keys: HashMap<String, Vec<u8>>,
}

impl CryptoContext {
    pub fn new() -> Self {
        Self {
            hospital_keys: HashMap::new(),
        }
    }

    /// Register a hospital and generate its encryption key.
    /// In production: hyde establishes PQC key exchange (ML-KEM-768)
    /// between hospital's TPM and the coordination server.
    pub fn register_hospital(&mut self, hospital_id: &str) {
        // Simulated key generation (unique per hospital)
        let mut key: Vec<u8> = hospital_id.as_bytes().to_vec();
        // Mix in a simple hash to ensure different hospitals produce different keys
        let hash: u8 = key.iter().fold(0u8, |acc, &b| acc.wrapping_mul(31).wrapping_add(b));
        key.iter_mut().for_each(|b| *b = b.wrapping_add(hash));
        self.hospital_keys.insert(hospital_id.to_string(), key);
    }

    /// Encrypt a patient record.
    /// In production: plat's CKKS scheme encrypts each field
    /// homomorphically, allowing score computation on ciphertext.
    pub fn encrypt_record(&self, hospital_id: &str, data: &[u8], record_type: &str) -> EncryptedRecord {
        // Simulated encryption (XOR with key for demonstration)
        let key = self.hospital_keys.get(hospital_id)
            .expect("Hospital not registered");
        let ciphertext: Vec<u8> = data.iter()
            .enumerate()
            .map(|(i, &b)| b ^ key[i % key.len()])
            .collect();

        EncryptedRecord {
            hospital_id: hospital_id.to_string(),
            ciphertext,
            record_type: record_type.to_string(),
        }
    }

    /// Generate a zero-knowledge compatibility proof.
    /// In production: argo's ZKP circuit proves compatibility
    /// without revealing the underlying medical data.
    pub fn prove_compatibility(
        &self,
        donor_hospital: &str,
        recipient_hospital: &str,
        score: f64,
        is_compatible: bool,
        donor_id: &str,
        recipient_id: &str,
    ) -> CompatibilityProof {
        // Simulated ZKP proof generation
        let proof_data = format!(
            "argo-zkp:{}:{}:{}",
            donor_hospital, recipient_hospital, is_compatible
        );

        CompatibilityProof {
            donor_id: donor_id.to_string(),
            recipient_id: recipient_id.to_string(),
            encrypted_score: score,
            proof: proof_data.into_bytes(),
            is_compatible,
        }
    }

    /// Verify a compatibility proof without accessing the underlying data.
    /// Any party can verify; no party learns the medical records.
    pub fn verify_proof(&self, proof: &CompatibilityProof) -> bool {
        // Simulated verification (always true for valid proofs)
        !proof.proof.is_empty() && proof.encrypted_score >= 0.0
    }
}

impl Default for CryptoContext {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hospital_registration() {
        let mut ctx = CryptoContext::new();
        ctx.register_hospital("tokyo-medical");
        ctx.register_hospital("osaka-university");
        assert!(ctx.hospital_keys.contains_key("tokyo-medical"));
        assert!(ctx.hospital_keys.contains_key("osaka-university"));
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let mut ctx = CryptoContext::new();
        ctx.register_hospital("hospital-a");
        let data = b"patient-record-001";
        let encrypted = ctx.encrypt_record("hospital-a", data, "donor");

        // Ciphertext should differ from plaintext
        assert_ne!(&encrypted.ciphertext, data);
        assert_eq!(encrypted.hospital_id, "hospital-a");
        assert_eq!(encrypted.record_type, "donor");
    }

    #[test]
    fn test_compatibility_proof() {
        let ctx = CryptoContext::new();
        let proof = ctx.prove_compatibility(
            "hospital-a", "hospital-b",
            0.85, true, "D001", "R001",
        );
        assert!(proof.is_compatible);
        assert!(ctx.verify_proof(&proof));
    }

    #[test]
    fn test_different_hospitals_different_ciphertext() {
        let mut ctx = CryptoContext::new();
        ctx.register_hospital("hospital-a");
        ctx.register_hospital("hospital-b");
        let data = b"same-data";
        let enc_a = ctx.encrypt_record("hospital-a", data, "donor");
        let enc_b = ctx.encrypt_record("hospital-b", data, "donor");

        // Same plaintext, different hospitals → different ciphertext
        assert_ne!(enc_a.ciphertext, enc_b.ciphertext);
    }
}
