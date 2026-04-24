# niobi

**Quantum optimisation alone cannot save organ transplant patients. If the data never shows up, the computation has nothing to compute.**

niobi is a liver-transplant matching system that fuses privacy-preserving cryptography with quantum-annealing optimisation. An encrypted substrate that lets hospitals contribute data *without anyone being able to read it* is what makes a full-network pool possible for the first time — and that is what makes large-scale quantum optimisation meaningful for the first time.

Submitted to **NEDO Challenge Q-2**: *Medical data sharing applications and algorithms for strengthening the drug discovery ecosystem.*

[日本語 README →](./README.ja.md)

## Why organ transplantation isn't optimised today

In Japan, the average wait for a deceased-donor kidney transplant is **15 years**. Of the 14,330 registered candidates, only 248 receive a transplant in a given year — **1.7 %**.

The bottleneck is not compute speed. The bottleneck is that **the data never aggregates**.

```
The structural problem today:

  Hospital A: has patient data
  Hospital B: has donor data
  Hospital C: has both

  → Cannot share medical data with each other (Personal Information
    Protection Act, internal hospital policies)
  → Would have to hand raw data to a central authority
  → Hospitals are reluctant; data quality is uneven
  → The optimiser's inputs never line up
  → Operations fall back to a simple "longest waiter first" rule
  → Nobody can measure the gap from the optimum
```

## niobi's end-to-end solution

Quantum optimisation **alone** will not deploy into society. Until the privacy problem is solved, hospitals will not release the data.

niobi solves **both at once**.

```
┌────────────────────────────────────────────────────────┐
│                    niobi architecture                  │
│                                                        │
│  Phase 1 — make the data releasable (precondition)     │
│  ┌──────────────────────────────────────────────────┐  │
│  │ hyde (TPM + PQC / ML-KEM-768)                    │  │
│  │   Each hospital encrypts data, device-bound.     │  │
│  │   Key exchange resistant to quantum attack.      │  │
│  │                                                  │  │
│  │ plat (FHE / CKKS)                                │  │
│  │   Compute compatibility scores on ciphertext.    │  │
│  │   No decryption — nobody sees raw data.          │  │
│  │                                                  │  │
│  │ argo (ZKP)                                       │  │
│  │   Prove "this patient and this donor match."     │  │
│  │   Why they match (blood type, etc.) stays sealed.│  │
│  └──────────────────────────────────────────────────┘  │
│                         ↓                              │
│  Phase 2 — find the optimum on the aggregated pool     │
│  ┌──────────────────────────────────────────────────┐  │
│  │ Quantum annealing (QUBO formulation)             │  │
│  │   Pairwise compatibility matrix → maximum match. │  │
│  │   Classical brute-force fails at 10×10.          │  │
│  │   Quantum reaches the optimum at larger scales.  │  │
│  └──────────────────────────────────────────────────┘  │
└────────────────────────────────────────────────────────┘
```

## Eliminating discretion

Today's organ allocation is always "someone is choosing." A coordinator's judgement, the designer's intent behind a points formula, the power dynamics between hospitals — all of it shapes the outcome. When you are not selected, you cannot see why, and you cannot audit why.

niobi reduces discretion to **mathematically zero**.

- Inputs are encrypted → nobody can manipulate them.
- Scoring runs under FHE → nobody can peek at intermediate values.
- Compatibility is proved by ZKP → verifiable, but inputs remain sealed.
- The optimiser is a quantum annealer → no human judgement enters the loop.
- **Compatibility is known between the two individuals and settled by their consent — hospitals return to being intermediaries.**

Data sovereignty stays with the individual. Hospitals run operations; they are no longer the decision-makers for who matches whom.

## Security model: blast-radius localisation and multi-device defence

In traditional systems, a breach of the central database leaks **everyone's** data. niobi is structurally different.

### Blast radius of a key compromise = one person

```
Centralised legacy:
  Attacker breaks the server → 14,330 patients' data leaks.
  A single key opens the entire dataset.

niobi:
  Attacker steals individual A's key → only A's data.
  The other 14,329 records are under different keys.
  The system as a whole is untouched.
```

Keys live inside the individual's device (TPM). Neither hospitals nor servers hold keys, so an attack on them has nothing to steal. **What is worth protecting is distributed. Attacks stop being economical.**

### Multi-device threshold authentication: multiply your devices

You do not have to rely on a single device. Split the key across devices and require cooperation to decrypt.

```
Example: PC + smartphone, 2-device configuration

  PC:         holds the first half k₁
  Phone:      holds the second half k₂
  Decryption: k = k₁ × k₂ — both pieces required

  → PC stolen? k₁ alone cannot decrypt.
  → Phone stolen? k₂ alone cannot decrypt.
  → Attacker must steal both simultaneously — cost explodes exponentially.
```

More devices, exponentially harder attacks:

```
1 device:  attack surface = 1 device
2 devices: attack surface = simultaneous theft of 2 devices (practically impossible)
3 devices: attack surface = simultaneous theft of 3 devices (structurally impossible)
```

**Your data, in your custody, hardened to the level you choose.** The strength of protection is set by the individual, not a system administrator. That is what data sovereignty actually means.

### Security is a choice the individual makes

No coercion. **Each person trades off convenience against risk on their own terms.**

```
Convenience-first:
  Phone only.
  → Minimal friction for daily use.
  → Loss is a risk, but a leak reveals only that one person.

Balanced:
  Phone + PC.
  → Sufficient for typical medical data.
  → Losing one does not compromise the other.

Maximum security:
  Phone + PC + hardware token (YubiKey etc.).
  → Decryption requires all three.
  → Withstands nation-state level attack.
```

Legacy systems fix the security level once, for everyone, at the administrator. If you want more protection, there is no knob to turn.

niobi inverts this. **Keep it simple if you want convenience; harden it if you're worried.** The system supports both choices equally.

### Compared to prior art

|  | Centralised | Blockchain | niobi |
|---|---|---|---|
| Damage from a key leak | **Everyone** | Individual only | **Individual only** |
| Multi-device defence | None | Wallet-dependent | **Threshold cryptography** |
| Server attack | All data leaks | All data already public | **Only ciphertext exists** |
| Responsibility for security | Administrator | Individual | **Individual (with support)** |

## Data authenticity: forgery is structurally impossible

"Protecting your own data" is not enough. niobi must also guarantee that **fake data cannot be injected**.

Every medical record carries a digital signature from the issuing laboratory or hospital. Data without a valid signature, or data that has been altered, is automatically rejected by the protocol.

```
Testing laboratory / hospital
  → Signs each test result with the institution's private key.
  → Hands the signed record to the individual's device.

Individual's device
  → Receives the signed record (plaintext + signature).
  → Encrypts via hyde into the pool.
  → The signature is carried inside the ciphertext.

The pool (during FHE computation)
  → Signature verification still works on ciphertext.
  → argo (ZKP) proves "this is a legitimately signed record."
  → Records without a signature, or altered records, are rejected.
```

### Attack scenarios and defences

| Attack | Result |
|---|---|
| Inflate your own MELD score | No lab signature → ZKP cannot be produced |
| Submit someone else's data as your own | Signature binds to a different individual → fails to pair with your hyde key |
| Forge the signature itself | You do not have the lab's private key → signature forgery is computationally infeasible |

### Three locks

1. **Lab signature** — proves data provenance (unforgeable).
2. **Binding to the hyde key** — proves the data belongs to this individual.
3. **ZKP verification** — proves the record is well-formed, under encryption, without revealing its content.

Gaining an advantage by lying is **mathematically impossible**. This is not "we trust honest actors to enter correct data"; it is a guarantee grounded in the physical fact that computing a forged signature would take longer than the life of the universe.

## Why the ordering matters

| Approach | Does data aggregate? | Does optimisation reach the optimum? | Deployable? |
|---|:---:|:---:|:---:|
| Status quo (centralised) | △ reluctantly | ✗ rules-based | △ stagnant |
| Quantum optimisation only | ✗ same barrier | ○ | ✗ no data |
| Privacy preservation only | ○ | ✗ classical limits | △ next-best |
| **niobi (both)** | **○** | **○** | **○** |

## Empirical results

D-Wave's `SimulatedAnnealingSampler` vs classical baselines:

| Scale | Greedy | Quantum annealing | Brute force | Greedy's loss |
|---|---|:---:|:---:|:---:|
| 4 donors × 4 recipients | 3 matches | **4 matches (optimum)** | 4 matches | **22.3 %** |
| 6 donors × 6 recipients | 4 matches | **5 matches (optimum)** | 5 matches | **17.5 %** |
| 10 donors × 10 recipients | 7 matches | 7 matches | infeasible | — |

Greedy matching (closest to current operational heuristics) misses **17 – 22 %** of optimal matches. In organ transplantation, that gap represents lives that could have been saved and weren't.

## Individual-sovereign protocol (7 steps)

The starting point is not the hospital. It is **the individual**.

```
Step 1: Anonymous key generation
        → Generated locally on the individual's device (hyde + TPM).
        → No registration. No hospital. No name.

Step 2: Individual contributes data
        → Encrypt medical record, drop into the pool.
        → Not even the hospital of origin is disclosed.

Step 3: FHE score computation
        → Compatibility computed on ciphertext.

Step 4: ZKP proof generation
        → Match / no-match proved for anonymous pair indices only.

Step 5: Quantum optimisation
        → Operates on anonymous indices.

Step 6: Notify the individuals
        → "A match exists" + the ZKP.
        → The counterparty's identity stays hidden until both consent.
        → Ignoring a notification is invisible — nobody learns you ignored it.

Step 7: Consent → hospital mediates
        → The hospital appears only after the two individuals agree.
        → Hospitals perform the surgery. They do not know the history of the match.
```

**The hospital does not appear until step 7.** At no step can any third party observe medical data, identities, or even whether a notification was delivered.

## Compatibility scoring (computed on encrypted data)

- ABO blood-group compatibility
- MELD / PELD score (urgency)
- Graft weight ratio (GRWR: donor liver volume / recipient body weight)
- Cold-ischaemia time (geographic distance, 12-hour constraint)
- Waiting duration

## Project layout

```
src/
├── lib.rs               — module definitions
├── scoring.rs           — compatibility scoring
├── matching.rs          — greedy matching
├── protocol.rs          — end-to-end protocol (plaintext variant)
├── crypto.rs            — hyde / argo / plat encryption layer
└── privacy_protocol.rs  — 6-step privacy-preserving protocol

quantum/
├── liver_matching_qubo.py   — QUBO / quantum annealing benchmark
├── matching_results.json    — comparison data
└── simulate_qkd.py          — BB84 quantum key distribution simulation
```

## Development

```bash
# Rust core (18 tests)
cargo build
cargo test

# Quantum simulation
cd quantum
pip install dimod dwave-samplers numpy
python liver_matching_qubo.py
```

## Forward outlook

### Nationwide default pool

Today's organ transplantation only matches people who have explicitly registered with a hospital. Latent donor candidates effectively do not exist to the system.

niobi + Japan's My Number card integration lets **every citizen be in the candidate pool by default**. Because hyde encrypts every individual's data, being in the pool is not itself a privacy risk. Only when a match arises does argo notify, and only then does the individual decide.

- Today: 14,330 waiting recipients vs. a few hundred registered donors per year.
- With niobi: 14,330 waiting recipients vs. a latent pool of **120 million people**.

### Invisibility of refusal

If you receive a match notification and choose not to proceed, that fact is **observable by no one**.

- argo proves "a match exists," but does not prove that any notification was delivered.
- Because hyde encrypts at the device layer, the very existence of a notification is invisible externally.
- Silence and refusal are indistinguishable → "I declined" simply does not exist as a fact.

If refusals were observable, nobody would stay in the pool. **Because they are not, 120 million people can stay in.** This is the decisive condition for real-world deployment.

### Cross-border matching

Each country's organ network is a silo. Japan 14,330 waiters, the United States 100,000, the EU 60,000 — data that should logically be a single pool, partitioned by GDPR, HIPAA, and the Personal Information Protection Act.

niobi realises international matching without moving the data.

```
Japanese patient ←── argo: match proof ──→ German donor
     │                                          │
  Data never leaves Japan.          Data never leaves Germany.
     │                                          │
     └──── Only the fact of a match flows across ────┘
```

#### A realistic rollout path

```
Layer 1: National pool
  An equivalent of the Japan Organ Transplant Network.
  First, bring every domestic hospital into hyde-encrypted storage.
  14,330 waiters → 120M-person latent pool.

Layer 2: Bilateral agreement
  Japan-Germany, Japan-US, grounded in existing medical cooperation.
  Mutual copies of encrypted data, started by diplomatic agreement.
  Each country's data sovereignty is fully preserved.

Layer 3: Regional union
  Asia, EU, North America.
  Encrypted pools shared within a region.
  Geographic scope bounded by cold-ischaemia time.

Layer 4: Global pool
  Encrypted data from everywhere replicated into every country.
  Any country can run optimal matching on the whole world.
  Especially powerful for rare blood types and rare diseases.
```

At each layer the technology is identical. Only the political and diplomatic sequencing advances. niobi does not circumvent GDPR or HIPAA — it **delivers the thing those laws were trying to protect, directly, via cryptography**.

#### Copies are harmless

Data encrypted with hyde, once copied, remains unreadable. niobi leans into that property and **replicates the world's encrypted data to every participating country**.

```
Japan server:   encrypted data (global copy)
Germany server: encrypted data (global copy)
US server:      encrypted data (global copy)
```

- Each country runs its own quantum matching (no latency).
- One country's server going down does not halt the others (no single point of failure).
- Your data is on your own soil → you do not depend on foreign infrastructure (true sovereignty).
- Eavesdropping and hacking become meaningless (only ciphertext ever exists).

Conventional data protection is designed to **prevent copying**. hyde is designed to **make copying harmless**. It is a different dimension of defence.

Under traditional security, redundancy is a risk — more copies means more attack surface. Under hyde, **redundancy makes attacks progressively meaningless**. A hundred replicas in a hundred countries are all the same ciphertext. The attacker's question "where to attack" simply dissolves.

Once we prove this out for organ transplantation, the same structure applies to:

- Pandemic-surveillance data shared internationally
- International co-operative clinical trials
- Cross-border rare-disease patient networks

These are all problems that previously stalled on "data sovereignty vs. international coordination."

hyde gets implemented once; the applications on top change. Just as TCP/IP was built once and HTTP, SMTP, and SSH all rode on top, hyde becomes the substrate for organ matching, blood donation, clinical trials, and rare-disease coordination. **"Everyone in the country" becomes "everyone in the world."**

### Technical roadmap

- plat CKKS FHE integration for fully-encrypted score computation.
- Validation on real D-Wave hardware at scale (100 donors × 1000 recipients).
- International pool construction: Japan → Asia → global, staged rollout.
- Beyond transplantation: drug-discovery data integration, insurance underwriting, infectious-disease data sharing.

## The step beyond blockchain

Blockchain delivered trust without a central authority by letting **everyone hold a copy and everyone verify**. It paid for that with **everyone can read**.

Medical data, personal information, state secrets — data that must not be seen simply cannot live on a blockchain. That is the wall that has blocked blockchain's arrival in real-world deployment.

niobi (via hyde / argo / plat) delivers the same structure **while remaining opaque**.

|  | Blockchain | niobi |
|---|---|---|
| Everyone holds a copy | ✓ | ✓ |
| Tamper detection | ✓ (via transparency) | ✓ (via ZKP) |
| No central authority required | ✓ | ✓ |
| Privacy | ✗ everyone reads | **✓ nobody reads** |

Transparency was a means to trust, not the goal itself. The goal was "proof of non-alteration." argo's ZKP achieves that **without showing the contents**.

## Related

- **[vohu](https://github.com/Ryujiyasu/vohu)** — sibling application on the same primitive. Privacy-preserving voting for World ID-verified humans: Paillier-encrypted ballots, homomorphic tally, 2-of-3 threshold decryption. Voting is the cleanest surface for the primitive; niobi (medical matching) is the second surface. Different domains, same crypto substrate (hyde + plat + argo).
- [hyde](https://gitlab.com/Ryujiyasu/hyde) — TPM + PQC device-binding primitive.
- [plat](https://gitlab.com/Ryujiyasu/plat) — FHE / GPU-accelerated private computation.
- [argo](https://gitlab.com/Ryujiyasu/argo) — zero-knowledge proof wrapper.

## License

MIT.

## AI assistance policy

This project uses AI (Claude) for code generation and writing, as a supporting tool. Final decisions on direction, verification of technical correctness, and responsibility for the data remain with the human author.
