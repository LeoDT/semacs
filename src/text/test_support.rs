// SPDX-License-Identifier: Apache-2.0
// RandomCharIter extracted from Zed util; adapted for semacs on 2026-10-31.
// Seed runner is semacs code. See THIRD_PARTY.md.

use rand::prelude::*;

pub struct RandomCharIter<T: Rng> {
    rng: T,
    simple_text: bool,
}

impl<T: Rng> RandomCharIter<T> {
    pub fn new(rng: T) -> Self {
        Self {
            rng,
            simple_text: std::env::var("SIMPLE_TEXT").is_ok_and(|v| !v.is_empty()),
        }
    }
}

impl<T: Rng> Iterator for RandomCharIter<T> {
    type Item = char;

    fn next(&mut self) -> Option<Self::Item> {
        if self.simple_text {
            return if self.rng.random_range(0..100) < 5 {
                Some('\n')
            } else {
                Some(self.rng.random_range(b'a'..b'z' + 1).into())
            };
        }

        match self.rng.random_range(0..100) {
            // whitespace
            0..=19 => [' ', '\n', '\r', '\t'].choose(&mut self.rng).copied(),
            // two-byte greek letters
            20..=32 => char::from_u32(self.rng.random_range(('α' as u32)..('ω' as u32 + 1))),
            // // three-byte characters
            33..=45 => ['✋', '✅', '❌', '❎', '⭐']
                .choose(&mut self.rng)
                .copied(),
            // // four-byte characters
            46..=58 => ['🍐', '🏀', '🍗', '🎉'].choose(&mut self.rng).copied(),
            // ascii letters
            _ => Some(self.rng.random_range(b'a'..b'z' + 1).into()),
        }
    }
}

pub fn run_seeds(default_iterations: u64, test: fn(StdRng)) {
    let start = std::env::var("SEED").map_or(0, |s| s.parse().expect("invalid SEED"));
    let iterations = std::env::var("ITERATIONS").map_or(default_iterations, |s| {
        s.parse().expect("invalid ITERATIONS")
    });
    for seed in start..start + iterations {
        eprintln!("seed = {seed}");
        test(StdRng::seed_from_u64(seed));
    }
}
