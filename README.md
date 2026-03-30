# qmed

Privacy-preserving liver transplant matching using homomorphic encryption
and quantum-safe cryptography.

NEDO Challenge Q-2: 創薬エコシステムの強化に向けた医療データ共有アプリケーション・アルゴリズムの開発

## Problem

Liver transplant matching requires sharing sensitive medical data (blood type,
MELD score, organ size, HLA typing, etc.) across hospitals. Current systems
require a trusted central authority with access to all plaintext records.

qmed eliminates this trust requirement.

## Approach: Hyde × Argo × PLAT

```
Hospital A          Hospital B          Hospital C
(donor data)        (recipient data)    (recipient data)
     │                   │                   │
     ▼                   ▼                   ▼
┌─────────┐        ┌─────────┐        ┌─────────┐
│  Hyde    │        │  Hyde    │        │  Hyde    │
│ (encrypt │        │ (encrypt │        │ (encrypt │
│  & send) │        │  & send) │        │  & send) │
└────┬─────┘        └────┬─────┘        └────┬─────┘
     │                   │                   │
     └───────────┬───────┴───────────────────┘
                 ▼
          ┌─────────────┐
          │    PLAT      │
          │ (FHE compute │
          │  compat.     │
          │  scores)     │
          └──────┬───────┘
                 ▼
          ┌─────────────┐
          │    Argo      │
          │ (encrypted   │
          │  optimal     │
          │  matching)   │
          └──────┬───────┘
                 ▼
          ┌─────────────┐
          │  Match Result │
          │  (only final  │
          │  assignment   │
          │  revealed)    │
          └──────────────┘
```

1. Each hospital encrypts patient/donor records locally via **Hyde**
2. **PLAT** (CKKS FHE) computes compatibility scores on ciphertext
3. **Argo** solves the assignment problem on encrypted scores
4. Only the final donor-recipient pairing is decrypted — no medical data leaves the source

## Key Innovation

- **No trusted third party**: The matching coordinator never sees raw patient data
- **Quantum-safe**: Lattice-based key exchange ensures long-term security of medical records
- **Verifiable**: Matching correctness can be audited without exposing inputs

## Components

- `src/` — Rust: FHE-based compatibility scoring and encrypted matching protocol
- `quantum/` — Python: Quantum KEM simulation, post-quantum key exchange verification
- `tests/` — Correctness and privacy tests
- `examples/` — Simulated multi-hospital matching scenarios

## Compatibility Scoring (encrypted)

Key factors computed homomorphically:
- Blood type (ABO) compatibility matrix
- MELD/PELD severity score ranking
- Organ size matching (donor-recipient body surface area)
- Geographic proximity (cold ischemia time constraint)
- Waiting time priority

## Development

```bash
cargo build
cargo test

cd quantum
pip install -r requirements.txt
python simulate.py
```

## License

MIT
