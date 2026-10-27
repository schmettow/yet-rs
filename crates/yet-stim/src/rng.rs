// SPDX-License-Identifier: GPL-3.0-only

//! Small deterministic PRNG for reproducible trial order.
//!
//! This is SplitMix64. It is not cryptographic; it exists so a session can be
//! replayed from its seed without adding a random-number dependency.

pub(crate) fn shuffled_indices(len: usize, seed: u64) -> Vec<usize> {
    let mut order: Vec<usize> = (0..len).collect();
    if len < 2 {
        return order;
    }

    let mut rng = SplitMix64::new(seed);
    for i in (1..len).rev() {
        let bound = i as u64 + 1;
        let j = (rng.next_u64() % bound) as usize;
        order.swap(i, j);
    }
    order
}

struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut value = self.state;
        value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        value ^ (value >> 31)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_sequences_are_unchanged() {
        assert_eq!(shuffled_indices(0, 42), Vec::<usize>::new());
        assert_eq!(shuffled_indices(1, 42), vec![0]);
    }

    #[test]
    fn shuffle_is_a_permutation() {
        let mut order = shuffled_indices(20, 42);
        order.sort_unstable();
        assert_eq!(order, (0..20).collect::<Vec<_>>());
    }

    #[test]
    fn shuffle_is_deterministic() {
        assert_eq!(shuffled_indices(20, 42), shuffled_indices(20, 42));
        assert_eq!(shuffled_indices(20, 7), shuffled_indices(20, 7));
    }

    #[test]
    fn different_seeds_can_produce_different_orders() {
        assert_ne!(shuffled_indices(20, 42), shuffled_indices(20, 7));
    }
}
