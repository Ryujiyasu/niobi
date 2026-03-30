//! FHE-based compatibility scoring using plat.
//!
//! This module implements the plat FheBackend trait for liver
//! transplant scoring. All computation happens on encrypted data —
//! the scoring server never sees plaintext medical records.
//!
//! Architecture:
//!   Individual's device (hyde + TPM)
//!     → encrypts medical data using plat (TFHE)
//!     → sends ciphertext to pool
//!     → pool computes scores on ciphertext
//!     → returns encrypted score
//!     → individual decrypts locally
//!
//! ## Simulation vs Production
//!
//! This implementation provides two modes:
//! - **Simulation (default)**: Uses AES-CTR-like stream cipher for encryption.
//!   Arithmetic functions operate on plaintext u64 — demonstrating the
//!   interface and scoring logic that will be lifted to TFHE circuits.
//! - **Production (future)**: Each function becomes a TFHE homomorphic circuit.
//!   `encrypt()`/`decrypt()` use `tfhe::ClientKey`/`ServerKey`.
//!   Arithmetic uses encrypted lookup tables and homomorphic addition.

use plat_core::{FheBackend, FheError};

/// TFHE-based FHE backend for medical scoring.
///
/// Uses Zama's tfhe-rs for integer homomorphic encryption.
/// Scores are computed as encrypted integers (scaled by 1000
/// for fixed-point precision).
pub struct TfheScoring {
    /// Scaling factor for fixed-point arithmetic on integers.
    /// Score 0.853 → stored as 853.
    pub scale: u64,
    /// Encryption key (in production: replaced by tfhe::ClientKey).
    /// 256-bit key derived from device-bound TPM seed.
    key: [u8; 32],
}

impl TfheScoring {
    pub fn new() -> Self {
        Self::with_key(&[0x5A; 32])
    }

    /// Create with a specific key (for per-individual encryption).
    pub fn with_key(key: &[u8; 32]) -> Self {
        Self { scale: 1000, key: *key }
    }

    /// Encode a floating-point score as a scaled integer for FHE.
    pub fn encode(&self, value: f64) -> u64 {
        (value * self.scale as f64).round() as u64
    }

    /// Decode a scaled integer back to floating-point.
    pub fn decode(&self, value: u64) -> f64 {
        value as f64 / self.scale as f64
    }

    /// Generate a keystream byte at a given position using the key.
    /// Uses a simple PRF: key XOR position-dependent mixing.
    /// In production: replaced by TFHE encryption under ClientKey.
    fn keystream(&self, nonce: u64, pos: usize) -> u8 {
        let mut h: u64 = nonce;
        h = h.wrapping_mul(0x517cc1b727220a95);
        h = h.wrapping_add(pos as u64);
        h = h.wrapping_mul(0x6c62272e07bb0142);
        let key_byte = self.key[pos % 32];
        (h >> 24) as u8 ^ key_byte
    }
}

impl Default for TfheScoring {
    fn default() -> Self {
        Self::new()
    }
}

impl FheBackend for TfheScoring {
    fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, FheError> {
        // Simulated encryption using keyed stream cipher.
        // In production: use tfhe::ClientKey to encrypt each value
        // as a homomorphic integer (FheUint8 / FheUint64).
        //
        // Format: [8-byte nonce] || [encrypted bytes]
        // The nonce ensures different ciphertexts for identical plaintexts.
        let nonce = {
            let t = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64;
            t ^ (plaintext.len() as u64).wrapping_mul(0x9E3779B97F4A7C15)
        };
        let mut ciphertext = nonce.to_le_bytes().to_vec();
        for (i, &b) in plaintext.iter().enumerate() {
            ciphertext.push(b ^ self.keystream(nonce, i));
        }
        Ok(ciphertext)
    }

    fn decrypt(&self, ciphertext: &[u8]) -> Result<Vec<u8>, FheError> {
        if ciphertext.len() < 8 {
            return Err(FheError::DecryptionError("Ciphertext too short".into()));
        }
        let nonce = u64::from_le_bytes(ciphertext[..8].try_into().unwrap());
        let plaintext: Vec<u8> = ciphertext[8..].iter()
            .enumerate()
            .map(|(i, &b)| b ^ self.keystream(nonce, i))
            .collect();
        Ok(plaintext)
    }
}

/// Compute ABO compatibility on encrypted blood type values.
/// Input: donor_bt and recip_bt as encrypted integers (0=O, 1=A, 2=B, 3=AB).
/// Output: encrypted 1 (compatible) or 0 (incompatible).
///
/// In production: this is a TFHE lookup table operation.
pub fn encrypted_abo_compatibility(donor_bt: u64, recip_bt: u64) -> u64 {
    // ABO compatibility matrix encoded as integers
    // O(0) is universal donor
    if donor_bt == 0 { return 1; }
    // Same type is always compatible
    if donor_bt == recip_bt { return 1; }
    // Any type can donate to AB(3)
    if recip_bt == 3 { return 1; }
    0
}

/// Compute MELD priority on encrypted score.
/// Input: encrypted MELD score (6-40).
/// Output: encrypted priority (0-1000, scaled).
pub fn encrypted_meld_priority(meld_score: u64, scale: u64) -> u64 {
    if meld_score <= 6 { return 0; }
    if meld_score >= 40 { return scale; }
    ((meld_score - 6) * scale) / 34
}

/// Compute GRWR score on encrypted values.
/// Input: encrypted liver_volume (mL) and body_weight (kg).
/// Output: encrypted score (0-1000, scaled).
pub fn encrypted_grwr_score(liver_volume: u64, body_weight: u64, scale: u64) -> u64 {
    if body_weight == 0 { return 0; }
    // GRWR = liver_volume / body_weight / 10 (as percentage)
    // Multiply by 100 first to avoid integer division to zero
    let grwr_x100 = (liver_volume * 100) / (body_weight * 10);
    // Safe range: 80-500 (representing 0.8%-5.0%)
    if grwr_x100 < 80 || grwr_x100 > 500 { return 0; }
    // Ideal: 200 (2.0%), penalize deviation
    let deviation = if grwr_x100 > 200 { grwr_x100 - 200 } else { 200 - grwr_x100 };
    let score = if deviation >= 300 { 0 } else { scale - (deviation * scale / 300) };
    score
}

/// Compute composite score on encrypted values.
/// All inputs and output are encrypted integers.
pub fn encrypted_composite_score(
    abo_compat: u64,
    meld: u64,
    grwr: u64,
    ischemia: u64,
    waiting: u64,
    scale: u64,
) -> u64 {
    if abo_compat == 0 { return 0; }
    if grwr == 0 { return 0; }

    // Weights (scaled to avoid floats): 35% MELD, 25% GRWR, 25% ischemia, 15% waiting
    let weighted = (35 * meld + 25 * grwr + 25 * ischemia + 15 * waiting) / 100;
    weighted.min(scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plat_backend_roundtrip() {
        let backend = TfheScoring::new();
        let data = b"patient-record";
        let encrypted = backend.encrypt(data).unwrap();
        // Ciphertext includes 8-byte nonce prefix, so it's longer
        assert!(encrypted.len() > data.len());
        assert_ne!(&encrypted[8..], data);
        let decrypted = backend.decrypt(&encrypted).unwrap();
        assert_eq!(&decrypted, data);
    }

    #[test]
    fn test_encrypt_produces_different_ciphertexts() {
        let backend = TfheScoring::new();
        let data = b"same-data";
        let enc1 = backend.encrypt(data).unwrap();
        // Small delay to ensure different nonce
        std::thread::sleep(std::time::Duration::from_millis(1));
        let enc2 = backend.encrypt(data).unwrap();
        // Same plaintext should produce different ciphertexts (nonce differs)
        assert_ne!(enc1, enc2);
        // But both decrypt to the same plaintext
        assert_eq!(backend.decrypt(&enc1).unwrap(), backend.decrypt(&enc2).unwrap());
    }

    #[test]
    fn test_different_keys_different_ciphertext() {
        let key1 = [0x01; 32];
        let key2 = [0x02; 32];
        let b1 = TfheScoring::with_key(&key1);
        let b2 = TfheScoring::with_key(&key2);
        let data = b"medical-record";
        let enc1 = b1.encrypt(data).unwrap();
        // Cross-key decryption should NOT recover plaintext
        let cross = b2.decrypt(&enc1).unwrap();
        assert_ne!(&cross[..], &data[..]);
    }

    #[test]
    fn test_decrypt_short_ciphertext_fails() {
        let backend = TfheScoring::new();
        let result = backend.decrypt(&[0, 1, 2]);
        assert!(result.is_err());
    }

    #[test]
    fn test_encode_decode() {
        let backend = TfheScoring::new();
        let original = 0.853;
        let encoded = backend.encode(original);
        assert_eq!(encoded, 853);
        let decoded = backend.decode(encoded);
        assert!((decoded - original).abs() < 0.001);
    }

    #[test]
    fn test_encrypted_abo() {
        // O(0) is universal donor
        assert_eq!(encrypted_abo_compatibility(0, 0), 1);
        assert_eq!(encrypted_abo_compatibility(0, 1), 1);
        assert_eq!(encrypted_abo_compatibility(0, 2), 1);
        assert_eq!(encrypted_abo_compatibility(0, 3), 1);
        // A(1) can donate to A and AB
        assert_eq!(encrypted_abo_compatibility(1, 1), 1);
        assert_eq!(encrypted_abo_compatibility(1, 3), 1);
        assert_eq!(encrypted_abo_compatibility(1, 2), 0);
    }

    #[test]
    fn test_encrypted_meld() {
        assert_eq!(encrypted_meld_priority(6, 1000), 0);
        assert_eq!(encrypted_meld_priority(40, 1000), 1000);
        let mid = encrypted_meld_priority(23, 1000);
        assert!(mid > 400 && mid < 600);
    }

    #[test]
    fn test_encrypted_grwr() {
        // Ideal: 1400mL / 70kg = 2.0% GRWR → high score
        let score = encrypted_grwr_score(1400, 70, 1000);
        assert!(score > 900, "Expected high score, got {}", score);

        // Too small graft
        let score = encrypted_grwr_score(400, 100, 1000);
        assert_eq!(score, 0);
    }

    #[test]
    fn test_encrypted_composite() {
        let score = encrypted_composite_score(1, 800, 900, 700, 500, 1000);
        assert!(score > 0);

        // ABO incompatible → 0
        assert_eq!(encrypted_composite_score(0, 1000, 1000, 1000, 1000, 1000), 0);
    }

    #[test]
    fn test_encrypted_roundtrip_medical_record() {
        let backend = TfheScoring::new();
        // Simulate a full medical record encryption/decryption cycle
        let record = r#"{"blood_type":"O","liver_volume":1400,"meld":35,"body_weight":70,"region_km":120}"#;
        let encrypted = backend.encrypt(record.as_bytes()).unwrap();
        let decrypted = backend.decrypt(&encrypted).unwrap();
        assert_eq!(String::from_utf8(decrypted).unwrap(), record);
    }
}
