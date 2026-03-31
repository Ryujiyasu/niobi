//! Multi-Key FHE scoring using plat-mkfhe.
//!
//! This module implements privacy-preserving liver transplant scoring
//! where each individual encrypts their data with their own independent key.
//! No individual surrenders their key. Scoring is computed homomorphically
//! across different keys. Decryption requires cooperative action by all
//! involved parties.
//!
//! Architecture:
//!   Individual A (patient):
//!     → generates own key pair (plat-mkfhe)
//!     → encrypts MELD score, waiting days under own key
//!     → sends ciphertext to pool
//!
//!   Individual B (donor):
//!     → generates own key pair (independently!)
//!     → encrypts ABO flag, GRWR score under own key
//!     → sends ciphertext to pool
//!
//!   Pool (no keys):
//!     → computes weighted score across ciphertexts from different keys
//!     → homomorphic addition + scalar multiplication (no decryption)
//!
//!   Both parties cooperate to decrypt the result.

use plat_core::ntt::NttTables;
use plat_core::params::Params;
use plat_mkfhe::{
    MkCiphertext, MkKeyPair, MkPublicKey, MkSecretKey,
    cooperative_decrypt, mk_add, mk_scalar_mul,
    mk_keys::mk_keygen,
};
use rand::Rng;

/// Parameters for MKFHE scoring.
/// Uses plat's test_small parameters for research prototype.
/// Production would use research_2048 or larger.
pub struct MkFheScoring {
    pub params: Params,
    pub ntt: NttTables,
}

impl MkFheScoring {
    /// Create scoring context with research parameters.
    pub fn new() -> Self {
        let params = Params::test_small();
        let ntt = NttTables::new(&params);
        Self { params, ntt }
    }

    /// Generate a key pair for an individual. Each individual calls this independently.
    pub fn keygen<R: Rng>(&self, party_id: u64, rng: &mut R) -> MkKeyPair {
        mk_keygen(party_id, &self.params, &self.ntt, rng)
    }

    /// Encrypt a u64 value under an individual's key.
    pub fn encrypt<R: Rng>(&self, pk: &MkPublicKey, value: u64, rng: &mut R) -> MkCiphertext {
        MkCiphertext::encrypt_u64(&self.params, &self.ntt, pk, value, rng)
    }

    /// Decrypt a multi-key ciphertext cooperatively.
    /// All involved parties must provide their secret keys.
    pub fn decrypt(&self, ct: &MkCiphertext, secret_keys: &[&MkSecretKey]) -> u64 {
        cooperative_decrypt(&self.params, &self.ntt, ct, secret_keys)
    }

    /// Homomorphic addition of two ciphertexts (may involve different keys).
    pub fn add(&self, ct1: &MkCiphertext, ct2: &MkCiphertext) -> MkCiphertext {
        mk_add(&self.params, ct1, ct2)
    }

    /// Multiply ciphertext by a plaintext scalar.
    pub fn scalar_mul(&self, ct: &MkCiphertext, scalar: u64) -> MkCiphertext {
        mk_scalar_mul(&self.params, ct, scalar)
    }

    /// Plaintext modulus (values must be in [0, t)).
    pub fn plaintext_modulus(&self) -> u64 {
        self.params.t
    }
}

impl Default for MkFheScoring {
    fn default() -> Self {
        Self::new()
    }
}

// --- Plaintext scoring functions (computed on client side) ---
// These are computed locally before encryption, so they operate on plaintext.
// ABO compatibility and GRWR are client-side because they involve
// branching/division which is expensive in FHE.

/// Compute ABO compatibility (client-side, before encryption).
/// Returns 1 (compatible) or 0 (incompatible).
pub fn abo_compatibility(donor_bt: u64, recip_bt: u64) -> u64 {
    if donor_bt == 0 { return 1; }     // O is universal donor
    if donor_bt == recip_bt { return 1; } // Same type
    if recip_bt == 3 { return 1; }     // Any → AB
    0
}

/// Compute MELD priority (client-side).
/// Returns scaled priority in [0, scale].
pub fn meld_priority(meld_score: u64, scale: u64) -> u64 {
    if meld_score <= 6 { return 0; }
    if meld_score >= 40 { return scale; }
    ((meld_score - 6) * scale) / 34
}

/// Compute GRWR score (client-side).
/// Returns scaled score in [0, scale].
pub fn grwr_score(liver_volume: u64, body_weight: u64, scale: u64) -> u64 {
    if body_weight == 0 { return 0; }
    let grwr_x100 = (liver_volume * 100) / (body_weight * 10);
    if grwr_x100 < 80 || grwr_x100 > 500 { return 0; }
    let deviation = if grwr_x100 > 200 { grwr_x100 - 200 } else { 200 - grwr_x100 };
    if deviation >= 300 { 0 } else { scale - (deviation * scale / 300) }
}

/// Compute ischemia score based on distance (client-side).
pub fn ischemia_score(distance_km: u64, scale: u64) -> u64 {
    let hours_x10 = distance_km * 10 / 100; // hours * 10
    if hours_x10 > 120 { return 0; } // > 12 hours
    scale - (hours_x10 * scale / 120)
}

/// Compute the composite score using MKFHE.
///
/// Patient (party 1) provides: encrypted MELD priority, encrypted waiting score
/// Donor (party 2) provides: encrypted ABO flag, encrypted GRWR score, encrypted ischemia
///
/// The weighted sum is computed homomorphically across different keys.
/// ABO gating (multiplication) is approximated by pre-multiplying on the donor side.
pub fn encrypted_composite_score(
    ctx: &MkFheScoring,
    ct_meld: &MkCiphertext,     // from patient (party 1)
    ct_waiting: &MkCiphertext,  // from patient (party 1)
    ct_grwr: &MkCiphertext,     // from donor (party 2)
    ct_ischemia: &MkCiphertext, // from donor (party 2)
) -> MkCiphertext {
    // Weighted sum: 35*meld + 25*grwr + 25*ischemia + 15*waiting
    // All operations are linear (scalar_mul + add), depth = 0
    let w_meld = ctx.scalar_mul(ct_meld, 7);      // 35/5 = 7
    let w_grwr = ctx.scalar_mul(ct_grwr, 5);       // 25/5 = 5
    let w_isch = ctx.scalar_mul(ct_ischemia, 5);   // 25/5 = 5
    let w_wait = ctx.scalar_mul(ct_waiting, 3);    // 15/5 = 3

    // Sum across different keys — this is where MKFHE shines
    let sum = ctx.add(&ctx.add(&w_meld, &w_grwr), &ctx.add(&w_isch, &w_wait));
    sum
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    #[test]
    fn test_single_party_roundtrip() {
        let ctx = MkFheScoring::new();
        let mut rng = StdRng::seed_from_u64(42);
        let kp = ctx.keygen(1, &mut rng);

        for val in 0..ctx.plaintext_modulus() {
            let ct = ctx.encrypt(&kp.public, val, &mut rng);
            let dec = ctx.decrypt(&ct, &[&kp.secret]);
            assert_eq!(val, dec, "roundtrip failed for {val}");
        }
    }

    #[test]
    fn test_cross_party_addition() {
        let ctx = MkFheScoring::new();
        let mut rng = StdRng::seed_from_u64(42);

        let patient = ctx.keygen(1, &mut rng);
        let donor = ctx.keygen(2, &mut rng);

        let ct1 = ctx.encrypt(&patient.public, 5, &mut rng);
        let ct2 = ctx.encrypt(&donor.public, 7, &mut rng);

        let ct_sum = ctx.add(&ct1, &ct2);

        // Both parties must cooperate
        let dec = ctx.decrypt(&ct_sum, &[&patient.secret, &donor.secret]);
        assert_eq!(dec, (5 + 7) % ctx.plaintext_modulus());
    }

    #[test]
    fn test_abo_compatibility() {
        assert_eq!(abo_compatibility(0, 0), 1); // O → O
        assert_eq!(abo_compatibility(0, 3), 1); // O → AB
        assert_eq!(abo_compatibility(1, 2), 0); // A → B
        assert_eq!(abo_compatibility(2, 1), 0); // B → A
    }

    #[test]
    fn test_meld_priority() {
        assert_eq!(meld_priority(6, 1000), 0);
        assert_eq!(meld_priority(40, 1000), 1000);
        let mid = meld_priority(23, 1000);
        assert!(mid > 400 && mid < 600);
    }

    #[test]
    fn test_grwr_score() {
        let score = grwr_score(1400, 70, 1000);
        assert!(score > 900, "Expected high score, got {score}");
        assert_eq!(grwr_score(400, 100, 1000), 0);
    }

    #[test]
    fn test_encrypted_composite_cross_party() {
        // THE KEY TEST: composite score across different keys
        let ctx = MkFheScoring::new();
        let mut rng = StdRng::seed_from_u64(42);
        let t = ctx.plaintext_modulus();

        let patient = ctx.keygen(1, &mut rng);
        let donor = ctx.keygen(2, &mut rng);

        // Patient data (encrypted under patient's key)
        let meld_val = 8u64 % t;
        let wait_val = 3u64 % t;
        let ct_meld = ctx.encrypt(&patient.public, meld_val, &mut rng);
        let ct_wait = ctx.encrypt(&patient.public, wait_val, &mut rng);

        // Donor data (encrypted under donor's key — different key!)
        let grwr_val = 5u64 % t;
        let isch_val = 4u64 % t;
        let ct_grwr = ctx.encrypt(&donor.public, grwr_val, &mut rng);
        let ct_isch = ctx.encrypt(&donor.public, isch_val, &mut rng);

        // Compute composite score homomorphically
        let ct_score = encrypted_composite_score(&ctx, &ct_meld, &ct_wait, &ct_grwr, &ct_isch);

        // Both parties cooperate to decrypt
        let dec = ctx.decrypt(&ct_score, &[&patient.secret, &donor.secret]);
        let expected = (7 * meld_val + 5 * grwr_val + 5 * isch_val + 3 * wait_val) % t;
        assert_eq!(dec, expected);
    }

    #[test]
    fn test_five_party_scenario() {
        // Five different individuals, each with their own key
        let ctx = MkFheScoring::new();
        let mut rng = StdRng::seed_from_u64(42);
        let t = ctx.plaintext_modulus();

        let keys: Vec<MkKeyPair> = (1..=5).map(|i| ctx.keygen(i, &mut rng)).collect();
        let values: Vec<u64> = vec![2, 3, 1, 4, 2];

        let cts: Vec<MkCiphertext> = keys.iter().zip(&values)
            .map(|(kp, &v)| ctx.encrypt(&kp.public, v % t, &mut rng))
            .collect();

        // Sum all five
        let mut total = cts[0].clone();
        for ct in &cts[1..] {
            total = ctx.add(&total, ct);
        }

        let sks: Vec<&MkSecretKey> = keys.iter().map(|kp| &kp.secret).collect();
        let dec = ctx.decrypt(&total, &sks);
        let expected: u64 = values.iter().sum::<u64>() % t;
        assert_eq!(dec, expected);
    }

    #[test]
    fn test_key_isolation() {
        // Party 2's key alone cannot decrypt a ciphertext involving party 1
        let ctx = MkFheScoring::new();
        let mut rng = StdRng::seed_from_u64(42);

        let kp1 = ctx.keygen(1, &mut rng);
        let kp2 = ctx.keygen(2, &mut rng);

        let ct1 = ctx.encrypt(&kp1.public, 5, &mut rng);
        let ct2 = ctx.encrypt(&kp2.public, 3, &mut rng);
        let ct_sum = ctx.add(&ct1, &ct2);

        // Only party 2 tries to decrypt — should give wrong result
        // (cooperative_decrypt only uses keys for parties in the ciphertext,
        //  but with only one party's key, the result is garbled)
        let dec_wrong = ctx.decrypt(&ct_sum, &[&kp2.secret]);
        let dec_right = ctx.decrypt(&ct_sum, &[&kp1.secret, &kp2.secret]);

        assert_eq!(dec_right, (5 + 3) % ctx.plaintext_modulus());
        assert_ne!(dec_wrong, dec_right, "single key should not decrypt correctly");
    }
}
