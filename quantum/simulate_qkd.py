"""
Quantum Key Distribution (BB84) simulation for niobi.

Simulates quantum-safe key exchange between hospitals.
The generated shared key is used to establish FHE parameter agreement
in the Hyde layer.

Runs on local quantum simulator (qiskit-aer) — no quantum hardware needed.
"""

from qiskit import QuantumCircuit
from qiskit_aer import AerSimulator
import numpy as np


def bb84_key_exchange(n_bits: int = 256, seed: int = 42) -> dict:
    """Simulate BB84 quantum key distribution.

    Args:
        n_bits: Number of raw qubits to transmit.
        seed: Random seed for reproducibility.

    Returns:
        Dictionary with sender/receiver bases, raw key, sifted key,
        and estimated QBER.
    """
    rng = np.random.default_rng(seed)

    # Sender (Hospital A) prepares random bits and bases
    sender_bits = rng.integers(0, 2, size=n_bits)
    sender_bases = rng.integers(0, 2, size=n_bits)  # 0=Z, 1=X

    # Receiver (Hospital B) chooses random measurement bases
    receiver_bases = rng.integers(0, 2, size=n_bits)

    # Quantum transmission simulation
    simulator = AerSimulator()
    receiver_bits = np.zeros(n_bits, dtype=int)

    for i in range(n_bits):
        qc = QuantumCircuit(1, 1)

        # Sender encodes
        if sender_bits[i] == 1:
            qc.x(0)
        if sender_bases[i] == 1:
            qc.h(0)

        # Receiver measures
        if receiver_bases[i] == 1:
            qc.h(0)
        qc.measure(0, 0)

        result = simulator.run(qc, shots=1, seed_simulator=seed + i).result()
        counts = result.get_counts()
        receiver_bits[i] = int(list(counts.keys())[0])

    # Sifting: keep only bits where bases match
    matching = sender_bases == receiver_bases
    sifted_sender = sender_bits[matching]
    sifted_receiver = receiver_bits[matching]

    # Estimate QBER (should be ~0 without eavesdropper)
    errors = np.sum(sifted_sender != sifted_receiver)
    qber = errors / len(sifted_sender) if len(sifted_sender) > 0 else 0.0

    return {
        "raw_bits": n_bits,
        "sifted_bits": len(sifted_sender),
        "sifted_key": sifted_sender.tolist(),
        "qber": qber,
        "secure": qber < 0.11,  # BB84 security threshold
    }


if __name__ == "__main__":
    print("=== niobi: BB84 QKD Simulation ===")
    print("Simulating key exchange between Hospital A and Hospital B...\n")

    result = bb84_key_exchange(n_bits=256)

    print(f"Raw qubits transmitted: {result['raw_bits']}")
    print(f"Sifted key length:      {result['sifted_bits']} bits")
    print(f"QBER:                   {result['qber']:.4f}")
    print(f"Key exchange secure:    {result['secure']}")
    print(f"\nFirst 32 bits of shared key: {result['sifted_key'][:32]}")
