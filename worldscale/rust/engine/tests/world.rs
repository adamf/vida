// Tests of the Rust world: the random numbers are Random123's, and the
// forest is the same however many ranks and cores it's run on.
//
//     cargo test --release -p worldscale-engine

use std::path::PathBuf;

use worldscale_engine::comm::{Comm, SerialComm, ThreadComm};
use worldscale_engine::forest::Forest;
use worldscale_engine::philox;
use worldscale_engine::settings::{self, Species, World};
use worldscale_engine::world::{Partition, RankWorld, Settings};

#[test]
fn philox_gives_random123s_known_answers() {
    let known = [
        ([0, 0, 0, 0], [0, 0], [0x6627E8D5, 0xE169C58D, 0xBC57AC4C, 0x9B00DBD8]),
        ([0xFFFFFFFF; 4], [0xFFFFFFFF, 0xFFFFFFFF], [0x408F276D, 0x41C83B0E, 0xA20BC7C6, 0x6D5451FD]),
        ([0x243F6A88, 0x85A308D3, 0x13198A2E, 0x03707344], [0xA4093822, 0x299F31D0],
         [0xD16CFE09, 0x94FDCCEB, 0x5001E420, 0x24126EA1]),
    ];
    for (counter, key, answer) in known {
        assert_eq!(philox::philox4x32(counter, key), answer);
    }
}

/// The top of the Vida repository (where Vida.py is)
fn vida_folder() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn species_and_world() -> (Species, World) {
    let vida = vida_folder();
    let species = Species::from_files(&settings::species_files_in(&vida.join("Species")).unwrap(), &vida).unwrap();
    let world = World::from_file(&vida, 60.0).unwrap();
    (species, world)
}

fn small_settings(partition: Partition, shuffle: bool) -> Settings {
    Settings {
        world_size: 60.0,
        tile_size: 15.0,
        seeds_per_hectare: 400.0,
        rng_start: 1,
        photon_limit: 200,
        partition,
        shuffle,
    }
}

/// Run a small world for 22 cycles on one rank; its fingerprint, and the
/// counts of the last cycle
fn run_one_rank(comm: &mut dyn Comm, settings: Settings) -> (String, u64, u64) {
    let (species, world) = species_and_world();
    let mut the_world = RankWorld::new(comm.rank(), comm.size(), settings, species, world).unwrap();
    let mut last = None;
    for _ in 0..22 {
        last = Some(the_world.run_cycle(comm).unwrap());
    }
    let last = last.unwrap();
    (the_world.fingerprint(comm).unwrap(), last.plants, last.born)
}

/// The same world on this many ranks (as threads)
fn run_ranks(ranks: usize, settings: Settings) -> String {
    let mut threads = Vec::new();
    for mut comm in ThreadComm::group(ranks) {
        let settings = settings.clone();
        threads.push(std::thread::spawn(move || run_one_rank(&mut comm, settings).0));
    }
    let fingerprints: Vec<String> = threads.into_iter().map(|thread| thread.join().unwrap()).collect();
    for fingerprint in &fingerprints {
        assert_eq!(fingerprint, &fingerprints[0], "the ranks should agree on the fingerprint");
    }
    fingerprints[0].clone()
}

#[test]
fn the_forest_is_the_same_on_any_number_of_ranks() {
    let (first, plants, born) = run_one_rank(&mut SerialComm, small_settings(Partition::Strips, false));
    assert!(plants > 100 && born > 10);
    assert_eq!(run_ranks(2, small_settings(Partition::Strips, false)), first);
    assert_eq!(run_ranks(3, small_settings(Partition::Curve, false)), first);
    assert_eq!(run_ranks(4, small_settings(Partition::Scattered, true)), first);
}

#[test]
fn the_forest_is_the_same_on_any_number_of_cores() {
    let mut fingerprints = Vec::new();
    for cores in [1, 2, 3, 8] {
        let pool = rayon::ThreadPoolBuilder::new().num_threads(cores).build().unwrap();
        fingerprints.push(pool.install(|| run_one_rank(&mut SerialComm, small_settings(Partition::Strips, false)).0));
    }
    for fingerprint in &fingerprints {
        assert_eq!(fingerprint, &fingerprints[0]);
    }
}

#[test]
fn a_different_start_for_the_random_numbers_gives_a_different_forest() {
    let first = run_one_rank(&mut SerialComm, small_settings(Partition::Strips, false)).0;
    let mut other = small_settings(Partition::Strips, false);
    other.rng_start = 2;
    assert_ne!(run_one_rank(&mut SerialComm, other).0, first);
}

#[cfg(not(feature = "system-maths"))]
#[test]
fn the_forest_is_the_same_as_the_python_prototypes() {
    // python -m worldscale.run -w 60 -tile 15 -t 22 -photons 200 -engine rust
    // (and -engine rust-world) give this fingerprint
    let (fingerprint, _, _) = run_one_rank(&mut SerialComm, small_settings(Partition::Strips, false));
    assert_eq!(fingerprint, "304:59ce747dc1db71b5d8f69dd041ef14ba");
}

/// A small random number generator for making test forests (splitmix64)
fn next_number(state: &mut u64) -> f64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64
}

#[test]
fn crushing_is_strongest_first_on_any_number_of_ranks() {
    // stems and seeds of all sizes, packed tight, so chains of overlaps
    // cross from tile to tile and rank to rank
    let count = 4000;
    let mut state = 7;
    let mut everything = Forest::zeros(count);
    for row in 0..count {
        everything.id[row] = (row as u64 * 2_654_435_761) % 1_000_003;
        everything.x[row] = next_number(&mut state) * 60.0 - 30.0;
        everything.y[row] = next_number(&mut state) * 60.0 - 30.0;
        let radius = if next_number(&mut state) < 0.8 { next_number(&mut state) * 0.3 } else { 0.3 + next_number(&mut state) * 2.5 };
        everything.is_seed[row] = next_number(&mut state) < 0.5;
        if everything.is_seed[row] {
            everything.radius_seed[row] = radius;
        } else {
            everything.radius_stem[row] = radius;
        }
        // (whole numbers, so some weigh the same)
        everything.mass_total[row] = (next_number(&mut state) * 20.0).floor();
        everything.birth_cycle[row] = (next_number(&mut state) * 3.0) as i32;
    }

    // the answer one tree at a time, strongest first
    let radius = |row: usize| everything.radius_seed[row].max(everything.radius_stem[row]);
    let mut order: Vec<usize> = (0..count).collect();
    order.sort_by(|&a, &b| {
        everything.mass_total[b].partial_cmp(&everything.mass_total[a]).unwrap()
            .then(everything.birth_cycle[a].cmp(&everything.birth_cycle[b]))
            .then(everything.id[a].cmp(&everything.id[b]))
    });
    let mut standing: Vec<usize> = Vec::new();
    let mut expected: Vec<u64> = Vec::new();
    for &row in &order {
        let crushed = standing.iter().any(|&other| {
            let dx = everything.x[row] - everything.x[other];
            let dy = everything.y[row] - everything.y[other];
            let reach = radius(row) + radius(other);
            dx * dx + dy * dy <= reach * reach
        });
        if crushed {
            expected.push(everything.id[row]);
        } else {
            standing.push(row);
        }
    }
    expected.sort_unstable();
    assert!(expected.len() > 500);

    for (ranks, partition) in [(1, Partition::Strips), (2, Partition::Strips), (3, Partition::Curve), (4, Partition::Scattered)] {
        let mut threads = Vec::new();
        for mut comm in ThreadComm::group(ranks) {
            let everything = everything.clone();
            threads.push(std::thread::spawn(move || {
                let (species, world) = species_and_world();
                let mut the_world = RankWorld::new(comm.rank(), comm.size(), small_settings(partition, false), species,
                                                   world).unwrap();
                let mine: Vec<usize> = (0..everything.len())
                    .filter(|&row| the_world.owner_of(everything.x[row], everything.y[row]) == comm.rank())
                    .collect();
                the_world.forest = everything.select(&mine);
                let (crushed, _) = the_world.crush(&mut comm).unwrap();
                let mut ids = Vec::new();
                for row in 0..crushed.len() {
                    if crushed[row] {
                        ids.push(the_world.forest.id[row]);
                    }
                }
                ids
            }));
        }
        let mut crushed: Vec<u64> = threads.into_iter().flat_map(|thread| thread.join().unwrap()).collect();
        crushed.sort_unstable();
        assert_eq!(crushed, expected, "on {} ranks", ranks);
    }
}
