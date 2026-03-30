// Web Worker: calls real Rust code compiled to WASM
// plat (FHE), argo (ZKP), simulated annealing — all Rust.

import init, { step1_keys, step2_encrypt, step3_score, step4_proofs, step5_matching } from './pkg/qmed_wasm.js';

let wasmReady = false;

async function ensureWasm() {
  if (!wasmReady) {
    await init();
    wasmReady = true;
  }
}

self.onmessage = async function(e) {
  const { nd, nr, sweeps, step } = e.data;
  await ensureWasm();

  if (step === 1) {
    const r = JSON.parse(step1_keys(nd, nr));
    self.postMessage({ type: 'keys', keys: r.keys, total: r.total });
  }
  else if (step === 2) {
    const r = JSON.parse(step2_encrypt(nd, nr));
    self.postMessage({
      type: 'encrypted',
      samples: r.samples.map(s => ({ id: s.id, plain: s.plaintext, cipher: s.ciphertext_hex })),
      totalRecords: r.total_records,
    });
  }
  else if (step === 3) {
    const r = JSON.parse(step3_score(nd, nr));
    self.postMessage({ type: 'scored', nCompat: r.n_compat, totalPairs: r.total_pairs, scoreSamples: r.score_samples });
  }
  else if (step === 4) {
    const r = JSON.parse(step4_proofs(nd, nr));
    self.postMessage({
      type: 'proofs', proofCount: r.proof_count,
      proofSamples: r.proof_samples.map(p => ({
        donor: p.donor, recip: p.recip, compatible: p.compatible, verified: p.verified,
        bucket: p.bucket, proofHex: p.proof_hex, proofBytes: p.proof_bytes,
      })),
    });
  }
  else if (step === 5) {
    // SA with progress callback from Rust
    const progressCb = (msg) => {
      self.postMessage({ type: 'sa_progress', msg });
    };
    const r = JSON.parse(step5_matching(nd, nr, sweeps, progressCb));
    self.postMessage({
      type: 'done',
      greedyMatches: r.greedy_matches, greedyScore: r.greedy_score,
      quantumMatches: r.quantum_matches, quantumScore: r.quantum_score,
      diff: r.diff, matchDetails: r.match_details,
    });
  }
};
