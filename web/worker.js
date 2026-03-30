// Niobi Web Worker: Rust/WASM で実行
// plat (FHE暗号化), argo (ZKP証明), 焼きなまし法 — 全てRust

import init, { step1_keys, step2_encrypt, step3_score, step4_proofs, step5_matching } from './pkg/niobi_wasm.js';

let wasmReady = false;
let currentSeed = 42;

async function ensureWasm() {
  if (!wasmReady) { await init(); wasmReady = true; }
}

self.onmessage = async function(e) {
  const { nd, nr, sweeps, step, seed } = e.data;
  if (seed !== undefined) currentSeed = seed;
  await ensureWasm();

  if (step === 1) {
    const r = JSON.parse(step1_keys(nd, nr, currentSeed));
    self.postMessage({ type: 'keys', keys: r.keys, total: r.total });
  }
  else if (step === 2) {
    const r = JSON.parse(step2_encrypt(nd, nr, currentSeed));
    self.postMessage({ type: 'encrypted', samples: r.samples, totalRecords: r.total_records });
  }
  else if (step === 3) {
    const r = JSON.parse(step3_score(nd, nr, currentSeed));
    self.postMessage({ type: 'scored', nCompat: r.n_compat, totalPairs: r.total_pairs, scoreSamples: r.score_samples });
  }
  else if (step === 4) {
    const r = JSON.parse(step4_proofs(nd, nr, currentSeed));
    self.postMessage({ type: 'proofs', proofCount: r.proof_count, proofSamples: r.proof_samples });
  }
  else if (step === 5) {
    const progressCb = (msg) => { self.postMessage({ type: 'sa_progress', msg }); };
    const r = JSON.parse(step5_matching(nd, nr, sweeps, currentSeed, progressCb));
    self.postMessage({
      type: 'done',
      greedyMatches: r.greedy_matches, greedyScore: r.greedy_score,
      quantumMatches: r.quantum_matches, quantumScore: r.quantum_score,
      diff: r.diff, matchDetails: r.match_details,
    });
  }
};
