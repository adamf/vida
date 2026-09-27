// Random numbers with addresses: Philox4x32-10, exactly as worldscale/philox.py
// works it out, so both give the same numbers for the same address.
//
// An address is packed into Philox's counter and key like this:
//     key     = rngStart (64 bits)
//     counter = tree id (64 bits), cycle (32 bits), purpose (8 bits) and
//               index (24 bits)

// What each random number is for (the same numbers as philox.py)
pub const PLACE_START: u32 = 1;
pub const GERMINATE: u32 = 2;
pub const FORM_SEED: u32 = 3;
pub const DISPERSE: u32 = 4;
pub const RANDOM_DEATH: u32 = 5;
pub const SLOW_GROWTH: u32 = 6;
pub const PHOTON: u32 = 7;

pub const INDEX_LIMIT: u32 = 1 << 24;

const MULTIPLIER_0: u64 = 0xD251_1F53;
const MULTIPLIER_1: u64 = 0xCD9E_8D57;
const KEY_STEP_0: u32 = 0x9E37_79B9;
const KEY_STEP_1: u32 = 0xBB67_AE85;

/// Philox4x32-10: ten rounds of multiplying and mixing a 128-bit counter
/// with a 64-bit key.
pub fn philox4x32(counter: [u32; 4], key: [u32; 2]) -> [u32; 4] {
    let mut c = counter;
    let mut k = key;
    for round in 0..10 {
        if round > 0 {
            // the key moves on between rounds (wrapping round at 2**32)
            k[0] = k[0].wrapping_add(KEY_STEP_0);
            k[1] = k[1].wrapping_add(KEY_STEP_1);
        }
        let product0 = (c[0] as u64) * MULTIPLIER_0;
        let product1 = (c[2] as u64) * MULTIPLIER_1;
        let high0 = (product0 >> 32) as u32;
        let low0 = product0 as u32;
        let high1 = (product1 >> 32) as u32;
        let low1 = product1 as u32;
        c = [high1 ^ c[1] ^ k[0], low1, high0 ^ c[3] ^ k[1], low0];
    }
    c
}

/// A 32-bit number as a number between 0 and 1 (never exactly 0 or 1)
fn between_0_and_1(word: u32) -> f64 {
    (word as f64 + 0.5) * (1.0 / 4_294_967_296.0)
}

/// The four random numbers at one address
pub fn random_block(rng_start: u64, id: u64, cycle: u32, purpose: u32, index: u32) -> [f64; 4] {
    let words = philox4x32(
        [id as u32, (id >> 32) as u32, cycle, (purpose << 24) | index],
        [rng_start as u32, (rng_start >> 32) as u32],
    );
    [
        between_0_and_1(words[0]),
        between_0_and_1(words[1]),
        between_0_and_1(words[2]),
        between_0_and_1(words[3]),
    ]
}
