// One rank's share of the world, and the cycle every rank does together:
// worldscale/world.py's TiledWorld, in Rust, giving exactly the same forest.
//
// The world is cut into square tiles. Which rank looks after which tiles is
// separate (strips, blocks along a space-filling curve, or scattered), and a
// tree belongs to the rank that has the tile it stands in. Each cycle:
//   1. germinate and grow: each tree on its own, from its own values
//   2. seeds that were thrown go to the rank of the tile they land in
//   3. deaths each tree decides for itself (random death, buckling...)
//   4. overlapping stems and seeds: the heavier crushes the lighter,
//      decided in rounds, swapping decisions with neighbouring ranks
//   5. shading, from a copy of the neighbours' plants near the tile edges
//   6. photosynthesis: each plant on its own
// Between the steps, ranks swap copies of the trees near their tiles' edges
// (the "halo"). Inside a rank, each step's work is shared out between the
// cores of the computer (rayon).
//
// Nothing depends on which rank or core does what, or in what order: every
// random number has an address, every decision about an overlap is made the
// same way everywhere, and trees get ids from their mother's tile. So any
// number of ranks, cores, and ways of sharing out the tiles give exactly the
// same forest; fingerprint() checks that.

use std::collections::HashMap;
use std::time::Instant;

use rayon::prelude::*;

use crate::comm::{Comm, Problem};
use crate::forest::{write_column, Forest, Reader, Wire};
use crate::growth::{self, Deaths, NewSeed, CRUSHED, LANDED_OFF_WORLD};
use crate::pairs;
use crate::philox;
use crate::settings::{Species, World};
use crate::shading;

/// A tree's id is (its tile's number << 40) | (how many were born in that
/// tile before it)
pub const TILE_BITS: u32 = 40;

/// The steps of a cycle, as the timings name them
pub const STEPS: [&str; 6] = ["grow", "send seeds", "own deaths", "crush", "shade", "photosynthesis"];

/// What's counted of the copying between ranks, over the whole run
pub const TRAFFIC: [&str; 4] = [
    "trees looked after",
    "halo copies for crushing",
    "halo copies for shading",
    "seeds sent to another rank",
];

/// How the tiles are shared out between the ranks
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Partition {
    /// in strips, a run of columns each
    Strips,
    /// along a Z-order (Morton) curve, cut into equal lengths
    Curve,
    /// each tile to a rank at random: the worst case for talking to neighbours
    Scattered,
}

impl Partition {
    pub fn from_name(name: &str) -> Result<Partition, String> {
        match name {
            "strips" => Ok(Partition::Strips),
            "curve" => Ok(Partition::Curve),
            "scattered" => Ok(Partition::Scattered),
            _ => Err("partition must be strips, curve or scattered".to_string()),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Partition::Strips => "strips",
            Partition::Curve => "curve",
            Partition::Scattered => "scattered",
        }
    }
}

/// What a run is: the world, the random numbers and how it's split up
/// (world.py's Settings)
#[derive(Clone, Debug)]
pub struct Settings {
    pub world_size: f64,
    pub tile_size: f64,
    pub seeds_per_hectare: f64,
    pub rng_start: u64,
    pub photon_limit: i64,
    pub partition: Partition,
    /// shuffle each rank's rows every cycle (it mustn't matter)
    pub shuffle: bool,
}

/// One cycle's counts, over the whole world
#[derive(Clone, Debug)]
pub struct Summary {
    pub cycle: u32,
    pub plants: u64,
    pub seeds: u64,
    pub born: u64,
    pub rounds: u32,
    /// in the order of growth::CAUSES
    pub deaths: [u64; 10],
}

pub struct RankWorld {
    pub rank: usize,
    pub ranks: usize,
    pub settings: Settings,
    pub species: Species,
    pub world: World,
    pub forest: Forest,
    pub cycle: u32,
    /// seconds spent in each of the STEPS
    pub timings: [f64; 6],
    /// the TRAFFIC counts
    pub traffic: [u64; 4],
    tiles_across: usize,
    half: f64,
    /// which rank has each tile (column * tiles_across + row)
    owner: Vec<u32>,
    /// how many seeds have been born in each of this rank's tiles
    born_in_tile: HashMap<u64, u64>,
    shuffler: u64,
}

// ---------------------------------------------------------------------
// Tiles
// ---------------------------------------------------------------------

/// A 32-bit number's bits spread out, one every other place (for the
/// Z-order curve)
fn interleave(value: u64) -> u64 {
    let mut bits = value & 0xFFFF_FFFF;
    bits = (bits | (bits << 16)) & 0x0000_FFFF_0000_FFFF;
    bits = (bits | (bits << 8)) & 0x00FF_00FF_00FF_00FF;
    bits = (bits | (bits << 4)) & 0x0F0F_0F0F_0F0F_0F0F;
    bits = (bits | (bits << 2)) & 0x3333_3333_3333_3333;
    (bits | (bits << 1)) & 0x5555_5555_5555_5555
}

/// Which rank looks after each tile (column * tiles_across + row)
pub fn tile_owners(tiles_across: usize, ranks: usize, partition: Partition) -> Vec<u32> {
    let count = tiles_across * tiles_across;
    let mut owner = vec![0u32; count];
    match partition {
        Partition::Strips => {
            for tile in 0..count {
                let column = tile / tiles_across;
                owner[tile] = (column * ranks / tiles_across) as u32;
            }
        }
        Partition::Curve => {
            // nearby tiles mostly go to the same rank
            let mut order: Vec<usize> = (0..count).collect();
            order.sort_by_key(|&tile| {
                let column = (tile / tiles_across) as u64;
                let row = (tile % tiles_across) as u64;
                interleave(column) | (interleave(row) << 1)
            });
            for (place, &tile) in order.iter().enumerate() {
                owner[tile] = (place as u64 * ranks as u64 / count as u64) as u32;
            }
        }
        Partition::Scattered => {
            for tile in 0..count {
                let column = (tile / tiles_across) as u32;
                let row = (tile % tiles_across) as u32;
                let mixed = philox::philox4x32([column, row, 0, 0], [12345, 678])[0];
                owner[tile] = (mixed as u64 % ranks as u64) as u32;
            }
        }
    }
    owner
}

/// Seeds scattered at random over these tiles, each tile's seeds worked out
/// from its own addresses (so a tile's seeds are the same whichever rank
/// makes them), with a random species for each seed (forest.startingSeeds)
fn starting_seeds(tiles: &[(usize, usize)], tiles_across: usize, tile_size: f64, seeds_per_tile: u64,
                  world_size: f64, species: &Species, rng_start: u64) -> Forest {
    let half = world_size / 2.0;
    let kinds = species.count();
    let place_seed = |tile: u64, corner_x: f64, corner_y: f64, number: u64| -> (u64, i32, f64, f64) {
        let block = philox::random_block(rng_start, tile, 0, philox::PLACE_START, number as u32);
        let x = corner_x + block[0] * tile_size;
        let y = corner_y + block[1] * tile_size;
        let kind = ((block[2] * kinds as f64) as i32).min(kinds as i32 - 1);
        ((tile << TILE_BITS) | number, kind, x, y)
    };
    let placed: Vec<(u64, i32, f64, f64)> = tiles
        .par_iter()
        .flat_map_iter(|&(column, row)| {
            let tile = (column * tiles_across + row) as u64;
            let corner_x = column as f64 * tile_size - half;
            let corner_y = row as f64 * tile_size - half;
            (0..seeds_per_tile).map(move |number| place_seed(tile, corner_x, corner_y, number))
        })
        // a tile only keeps seeds inside the world (tiles at the edge can stick out)
        .filter(|seed| seed.2 >= -half && seed.2 < half && seed.3 >= -half && seed.3 < half)
        .collect();
    let mut seeds = Forest::zeros(placed.len());
    for (row, &(id, kind, x, y)) in placed.iter().enumerate() {
        let s = kind as usize;
        seeds.id[row] = id;
        seeds.species[row] = kind;
        seeds.is_seed[row] = true;
        seeds.x[row] = x;
        seeds.y[row] = y;
        seeds.mass_seed[row] = species.mass_seed_max[s];
        seeds.radius_seed[row] = growth::seed_radius(seeds.mass_seed[row], species.density_seed[s]);
        seeds.r[row] = seeds.radius_seed[row];
        seeds.mass_total[row] = seeds.mass_seed[row];
    }
    seeds
}

/// Where a point is along a Z-order curve of 1 m squares. Rows are kept in
/// this order (each cycle's new seeds among themselves), so trees near each
/// other on the ground are mostly near each other in memory, and finding
/// neighbours doesn't keep waiting for memory. That's only for speed: the
/// rows' order never changes the answer.
fn place_on_curve(x: f64, y: f64, half: f64) -> u64 {
    let across = (x + half).clamp(0.0, u32::MAX as f64) as u64;
    let up = (y + half).clamp(0.0, u32::MAX as f64) as u64;
    interleave(across) | (interleave(up) << 1)
}

/// The next number from a small random number generator (splitmix64), for
/// shuffling rows (not part of the model)
fn next_random(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// One row's values scrambled into two 64-bit numbers (for fingerprint)
fn row_fingerprint(f: &Forest, row: usize) -> (u64, u64) {
    let values: [u64; 18] = [
        f.id[row],
        f.species[row] as i64 as u64,
        f.is_seed[row] as u64,
        f.x[row].to_bits(),
        f.y[row].to_bits(),
        f.birth_cycle[row] as i64 as u64,
        f.age[row] as i64 as u64,
        f.count_to_germ[row] as i64 as u64,
        f.mass_seed[row].to_bits(),
        f.mass_stem[row].to_bits(),
        f.mass_leaf[row].to_bits(),
        f.mass_fixed[row].to_bits(),
        f.height_stem[row].to_bits(),
        f.r[row].to_bits(),
        f.area_covered[row].to_bits(),
        f.is_mature[row] as u64,
        f.attached_count[row] as i64 as u64,
        f.attached_mass[row].to_bits(),
    ];
    let mut words = [0u32; 4];
    for value in values {
        words = philox::philox4x32(
            [words[0] ^ value as u32, words[1] ^ (value >> 32) as u32, words[2], words[3]],
            [0x9E37_79B9, 0x7F4A_7C15],
        );
    }
    (words[0] as u64 | ((words[1] as u64) << 32), words[2] as u64 | ((words[3] as u64) << 32))
}

impl RankWorld {
    /// This rank's share of a new world: the tiles it looks after, with
    /// their starting seeds
    pub fn new(rank: usize, ranks: usize, settings: Settings, species: Species, mut world: World)
               -> Result<RankWorld, Problem> {
        if !(settings.world_size > 0.0 && settings.tile_size > 0.0) {
            return Err("the world and its tiles must be bigger than nothing".into());
        }
        world.world_size = settings.world_size;
        let tiles_across = (settings.world_size / settings.tile_size).ceil() as usize;
        let owner = tile_owners(tiles_across, ranks, settings.partition);
        let half = settings.world_size / 2.0;
        // (Python's round: halves go to the even number)
        let seeds_per_tile =
            (settings.seeds_per_hectare * settings.tile_size * settings.tile_size / 10000.0).round_ties_even() as u64;
        if seeds_per_tile > philox::INDEX_LIMIT as u64 {
            return Err("too many starting seeds in a tile: use smaller tiles".into());
        }
        let mut my_tiles = Vec::new();
        let mut born_in_tile = HashMap::new();
        for column in 0..tiles_across {
            for row in 0..tiles_across {
                if owner[column * tiles_across + row] as usize == rank {
                    my_tiles.push((column, row));
                    born_in_tile.insert((column * tiles_across + row) as u64, seeds_per_tile);
                }
            }
        }
        let mut forest = starting_seeds(&my_tiles, tiles_across, settings.tile_size, seeds_per_tile,
                                        settings.world_size, &species, settings.rng_start);
        let mut places: Vec<(u64, usize)> =
            (0..forest.len()).map(|row| (place_on_curve(forest.x[row], forest.y[row], half), row)).collect();
        places.par_sort_unstable();
        forest.reorder(&places.iter().map(|&(_, row)| row).collect::<Vec<usize>>());
        Ok(RankWorld {
            rank,
            ranks,
            settings,
            species,
            world,
            forest,
            cycle: 0,
            timings: [0.0; 6],
            traffic: [0; 4],
            tiles_across,
            half,
            owner,
            born_in_tile,
            shuffler: 1000 + rank as u64,
        })
    }

    /// The tile a point is in (points off the world count as in the
    /// nearest tile)
    fn tile_of(&self, x: f64, y: f64) -> (usize, usize) {
        let last = self.tiles_across as i64 - 1;
        let column = (((x + self.half) / self.settings.tile_size).floor() as i64).clamp(0, last);
        let row = (((y + self.half) / self.settings.tile_size).floor() as i64).clamp(0, last);
        (column as usize, row as usize)
    }

    /// The rank that has the tile a point is in
    pub fn owner_of(&self, x: f64, y: f64) -> usize {
        let (column, row) = self.tile_of(x, y);
        self.owner[column * self.tiles_across + row] as usize
    }

    /// For each rank, which of these rows it needs a copy of: those within
    /// `width` of a tile it looks after (in the same order as `rows`).
    /// width must be no more than a tile, so only the 8 tiles round a tree's
    /// own can need it.
    fn halo_for(&self, rows: &[usize], width: f64) -> Result<Vec<Vec<usize>>, Problem> {
        let tile_size = self.settings.tile_size;
        if width > tile_size {
            return Err(format!("something reaches {:.1} m, more than a tile ({:.1} m): use bigger tiles (-tile)",
                               width, tile_size).into());
        }
        let mut sends = vec![Vec::new(); self.ranks];
        if self.ranks == 1 || rows.is_empty() {
            return Ok(sends);
        }
        let last = self.tiles_across as i64 - 1;
        let wanted_by = |row: usize, found: &mut Vec<(u32, usize)>| {
            let x = self.forest.x[row];
            let y = self.forest.y[row];
            let (column, tile_row) = self.tile_of(x, y);
            // only trees near an edge of their tile can be needed by another rank
            let inside_x = x + self.half - column as f64 * tile_size;
            let inside_y = y + self.half - tile_row as f64 * tile_size;
            let near_left = inside_x < width;
            let near_right = inside_x > tile_size - width;
            let near_bottom = inside_y < width;
            let near_top = inside_y > tile_size - width;
            if !(near_left || near_right || near_bottom || near_top) {
                return;
            }
            let mut ranks = [0u32; 8];
            let mut how_many = 0;
            for across in -1i64..=1 {
                for up in -1i64..=1 {
                    if across == 0 && up == 0 {
                        continue;
                    }
                    let other_column = column as i64 + across;
                    let other_row = tile_row as i64 + up;
                    if other_column < 0 || other_column > last || other_row < 0 || other_row > last {
                        continue;
                    }
                    if (across == -1 && !near_left) || (across == 1 && !near_right)
                        || (up == -1 && !near_bottom) || (up == 1 && !near_top) {
                        continue;
                    }
                    let who = self.owner[other_column as usize * self.tiles_across + other_row as usize];
                    if who as usize != self.rank && !ranks[..how_many].contains(&who) {
                        ranks[how_many] = who;
                        how_many += 1;
                    }
                }
            }
            for &who in &ranks[..how_many] {
                found.push((who, row));
            }
        };
        let wanted: Vec<(u32, usize)> = rows
            .par_chunks(4096)
            .flat_map_iter(|chunk| {
                let mut found = Vec::new();
                for &row in chunk {
                    wanted_by(row, &mut found);
                }
                found
            })
            .collect();
        for (who, row) in wanted {
            sends[who as usize].push(row);
        }
        Ok(sends)
    }

    fn time_step(&mut self, step: usize, started: Instant) -> Instant {
        let now = Instant::now();
        self.timings[step] += (now - started).as_secs_f64();
        now
    }

    /// Take out the rows that die
    fn drop_rows(&mut self, dying: &[bool]) {
        if dying.iter().any(|&gone| gone) {
            self.forest.keep_where(dying, false);
        }
    }

    // -----------------------------------------------------------------
    // One cycle
    // -----------------------------------------------------------------

    pub fn run_cycle(&mut self, comm: &mut dyn Comm) -> Result<Summary, Problem> {
        let cycle = self.cycle;
        let rng_start = self.settings.rng_start;
        let mut deaths = Deaths::default();
        let mut clock = Instant::now();

        // 1. germinate and grow, each tree on its own
        let mut dying = vec![false; self.forest.len()];
        let (grown, dispersing) = growth::germinate_and_grow(&mut self.forest.trees(), cycle, &self.species,
                                                             &self.world, rng_start, &mut dying);
        deaths = deaths + grown;
        let seeds = growth::disperse(&self.forest.trees(), &dispersing, cycle, &self.species, &self.world, rng_start);
        let ids = self.new_ids(&seeds);
        // seeds landing off the world die; the rest go to the rank that has
        // the tile they land in
        let mut going_to = vec![Vec::new(); self.ranks];
        for (place, seed) in seeds.iter().enumerate() {
            if seed.x >= -self.half && seed.x < self.half && seed.y >= -self.half && seed.y < self.half {
                going_to[self.owner_of(seed.x, seed.y)].push(place);
            } else {
                deaths.counts[LANDED_OFF_WORLD] += 1;
            }
        }
        // (each letter's seeds in order of where they land)
        let half = self.half;
        going_to.par_iter_mut().for_each(|places| {
            let mut keyed: Vec<(u64, usize)> =
                places.par_iter().map(|&place| (place_on_curve(seeds[place].x, seeds[place].y, half), place)).collect();
            keyed.par_sort_unstable();
            *places = keyed.into_iter().map(|(_, place)| place).collect();
        });
        let mut born = 0;
        for (rank, places) in going_to.iter().enumerate() {
            born += places.len() as u64;
            if rank != self.rank {
                self.traffic[3] += places.len() as u64;
            }
        }
        // (the rows that die here are taken out with step 3's)
        clock = self.time_step(0, clock);

        // 2. seeds go to the rank of the tile they land in
        let letters = going_to.iter().map(|places| seed_letter(&seeds, &ids, places, cycle)).collect();
        for letter in comm.alltoall(letters)? {
            read_seed_letter(&letter, &mut self.forest)?;
        }
        dying.resize(self.forest.len(), false);
        clock = self.time_step(1, clock);

        // 3. deaths each tree decides for itself
        let own = growth::own_deaths(&self.forest.trees(), cycle, &self.species, &self.world, rng_start, &mut dying);
        deaths = deaths + own;
        self.drop_rows(&dying);
        clock = self.time_step(2, clock);

        // 4. overlapping stems and seeds
        let mut rounds = 0;
        if !self.world.allow_overlaps {
            let (crushed, crush_rounds) = self.crush(comm)?;
            deaths.counts[CRUSHED] += crushed.iter().filter(|&&gone| gone).count() as u64;
            self.drop_rows(&crushed);
            rounds = crush_rounds;
        }
        clock = self.time_step(3, clock);

        // 5. shading
        self.forest.area_covered = self.shade(comm)?;
        clock = self.time_step(4, clock);

        // 6. photosynthesis
        let mut dying = vec![false; self.forest.len()];
        deaths = deaths + growth::photosynthesise(&mut self.forest.trees(), &self.species, &mut dying);
        self.drop_rows(&dying);
        self.time_step(5, clock);

        if self.settings.shuffle {
            self.shuffle_rows();
        }
        self.cycle = cycle + 1;
        self.summary(comm, &deaths, born, rounds)
    }

    /// Ids for seeds born this cycle: each mother's tile numbers its new
    /// seeds in order of (mother's id, seed number), carrying on from the
    /// last number it gave out. The same ids whichever rank has the tile.
    fn new_ids(&mut self, seeds: &[NewSeed]) -> Vec<u64> {
        let count = seeds.len();
        let tiles: Vec<u64> = seeds
            .par_iter()
            .map(|seed| {
                let (column, row) = self.tile_of(seed.mother_x, seed.mother_y);
                (column * self.tiles_across + row) as u64
            })
            .collect();
        let mut order: Vec<usize> = (0..count).collect();
        order.par_sort_unstable_by_key(|&place| (tiles[place], seeds[place].mother_id, seeds[place].seed_number));
        let mut ids = vec![0u64; count];
        let mut place = 0;
        while place < count {
            let tile = tiles[order[place]];
            let born = self.born_in_tile.entry(tile).or_insert(0);
            while place < count && tiles[order[place]] == tile {
                ids[order[place]] = (tile << TILE_BITS) | *born;
                *born += 1;
                place += 1;
            }
        }
        ids
    }

    // -----------------------------------------------------------------
    // 4. Overlapping stems and seeds
    // -----------------------------------------------------------------

    /// Vida's removeOverlaps, with a rule that doesn't depend on order: a
    /// tree survives unless it overlaps a stronger tree (heavier, or planted
    /// first) that survives. Decided in rounds: in each round a tree whose
    /// stronger neighbours have all been decided is decided (crushed if any
    /// of them survived, surviving if none did), and ranks swap the
    /// decisions about the trees near their edges, until nothing is left
    /// undecided. Gives back which rows are crushed, and how many rounds it
    /// took.
    pub fn crush(&mut self, comm: &mut dyn Comm) -> Result<(Vec<bool>, u32), Problem> {
        let count = self.forest.len();
        let forest = &self.forest;
        let radius: Vec<f64> = (0..count)
            .into_par_iter()
            .map(|row| if forest.is_seed[row] { forest.radius_seed[row] } else { forest.radius_stem[row] })
            .collect();
        let biggest = radius.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
        let largest = comm.max(if count > 0 { biggest } else { 0.0 })?;
        let rows: Vec<usize> = (0..count).collect();
        let sends = self.halo_for(&rows, 2.0 * largest * 1.000001 + 0.000001)?;

        // copies of the neighbours' trees near our tiles, after our own
        let letters = sends
            .iter()
            .map(|rows| {
                let mut letter = Vec::new();
                (rows.len() as u64).write(&mut letter);
                write_column(&mut letter, &forest.id, rows);
                write_column(&mut letter, &forest.x, rows);
                write_column(&mut letter, &forest.y, rows);
                write_column(&mut letter, &radius, rows);
                write_column(&mut letter, &forest.mass_total, rows);
                write_column(&mut letter, &forest.birth_cycle, rows);
                letter
            })
            .collect();
        let mut ids = forest.id.clone();
        let mut x = forest.x.clone();
        let mut y = forest.y.clone();
        let mut all_radius = radius;
        let mut mass = forest.mass_total.clone();
        let mut birth = forest.birth_cycle.clone();
        for letter in comm.alltoall(letters)? {
            let mut reader = Reader::new(&letter);
            let copies = reader.value::<u64>() as usize;
            reader.column(copies, &mut ids);
            reader.column(copies, &mut x);
            reader.column(copies, &mut y);
            reader.column(copies, &mut all_radius);
            reader.column(copies, &mut mass);
            reader.column(copies, &mut birth);
        }
        self.traffic[0] += count as u64;
        self.traffic[1] += (ids.len() - count) as u64;

        // each overlapping pair where the weaker is ours, and for each of
        // our trees, the stronger trees it overlaps
        let (winners, losers) = pairs::overlap_winners(&x, &y, &all_radius, &mass, &birth, &ids, count);
        let mut first_winner = vec![0usize; count + 1];
        for &loser in &losers {
            first_winner[loser as usize + 1] += 1;
        }
        for row in 0..count {
            first_winner[row + 1] += first_winner[row];
        }
        let mut winners_of = vec![0usize; losers.len()];
        let mut next = first_winner.clone();
        for pair in 0..losers.len() {
            let loser = losers[pair] as usize;
            winners_of[next[loser]] = winners[pair] as usize;
            next[loser] += 1;
        }

        const UNDECIDED: u8 = 0;
        const ALIVE: u8 = 1;
        const GONE: u8 = 2;
        let mut status = vec![UNDECIDED; ids.len()];
        let mut rounds = 0;
        loop {
            rounds += 1;
            let decide = |row: usize| -> u8 {
                if status[row] != UNDECIDED {
                    return status[row];
                }
                let mut killed = false;
                let mut waiting = false;
                for &winner in &winners_of[first_winner[row]..first_winner[row + 1]] {
                    if status[winner] == ALIVE {
                        killed = true;
                    } else if status[winner] == UNDECIDED {
                        waiting = true;
                    }
                }
                if killed {
                    GONE
                } else if waiting {
                    UNDECIDED
                } else {
                    ALIVE
                }
            };
            let decided: Vec<u8> = (0..count).into_par_iter().with_min_len(1024).map(decide).collect();
            status[..count].copy_from_slice(&decided);
            // swap the decisions about the trees near the edges
            let letters = sends.iter().map(|rows| rows.iter().map(|&row| status[row]).collect()).collect();
            let mut place = count;
            for letter in comm.alltoall(letters)? {
                status[place..place + letter.len()].copy_from_slice(&letter);
                place += letter.len();
            }
            let left = decided.iter().filter(|&&decision| decision == UNDECIDED).count() as u64;
            if comm.sum(&[left])?[0] == 0 {
                break;
            }
        }
        let crushed = status[..count].iter().map(|&decision| decision == GONE).collect();
        Ok((crushed, rounds))
    }

    // -----------------------------------------------------------------
    // 5. Shading
    // -----------------------------------------------------------------

    /// Each of our rows' shaded area
    fn shade(&mut self, comm: &mut dyn Comm) -> Result<Vec<f64>, Problem> {
        let forest = &self.forest;
        let count = forest.len();
        let plants: Vec<usize> = (0..count).filter(|&row| !forest.is_seed[row]).collect();
        let biggest = plants.iter().fold(f64::NEG_INFINITY, |a, &row| a.max(forest.r[row]));
        let largest = comm.max(if plants.is_empty() { 0.0 } else { biggest })?;
        let sends = self.halo_for(&plants, 2.0 * largest * 1.000001 + 0.000001)?;
        let letters = sends
            .iter()
            .map(|rows| {
                let mut letter = Vec::new();
                (rows.len() as u64).write(&mut letter);
                write_column(&mut letter, &forest.id, rows);
                write_column(&mut letter, &forest.x, rows);
                write_column(&mut letter, &forest.y, rows);
                write_column(&mut letter, &forest.r, rows);
                write_column(&mut letter, &forest.height_stem, rows);
                write_column(&mut letter, &forest.species, rows);
                letter
            })
            .collect();
        let mut ids = forest.id.clone();
        let mut x = forest.x.clone();
        let mut y = forest.y.clone();
        let mut r = forest.r.clone();
        let mut height = forest.height_stem.clone();
        let mut species = forest.species.clone();
        let mut is_plant: Vec<bool> = forest.is_seed.iter().map(|&seed| !seed).collect();
        for letter in comm.alltoall(letters)? {
            let mut reader = Reader::new(&letter);
            let copies = reader.value::<u64>() as usize;
            reader.column(copies, &mut ids);
            reader.column(copies, &mut x);
            reader.column(copies, &mut y);
            reader.column(copies, &mut r);
            reader.column(copies, &mut height);
            reader.column(copies, &mut species);
            is_plant.resize(is_plant.len() + copies, true);
        }
        self.traffic[2] += (ids.len() - count) as u64;
        Ok(shading::shade(count, &x, &y, &r, &is_plant, &height, &ids, &species, self.cycle, &self.species,
                          &self.world, self.settings.photon_limit, self.settings.rng_start))
    }

    /// Shuffle this rank's rows (to check that it doesn't matter)
    fn shuffle_rows(&mut self) {
        let count = self.forest.len();
        let mut order: Vec<usize> = (0..count).collect();
        for place in (1..count).rev() {
            let other = (next_random(&mut self.shuffler) % (place as u64 + 1)) as usize;
            order.swap(place, other);
        }
        self.forest.reorder(&order);
    }

    // -----------------------------------------------------------------
    // Counting and checking
    // -----------------------------------------------------------------

    fn summary(&self, comm: &mut dyn Comm, deaths: &Deaths, born: u64, rounds: u32) -> Result<Summary, Problem> {
        let seeds = self.forest.is_seed.iter().filter(|&&seed| seed).count() as u64;
        let plants = self.forest.len() as u64 - seeds;
        let mut counts = vec![plants, seeds, born];
        counts.extend_from_slice(&deaths.counts);
        let totals = comm.sum(&counts)?;
        let mut all_deaths = [0u64; 10];
        all_deaths.copy_from_slice(&totals[3..13]);
        Ok(Summary { cycle: self.cycle - 1, plants: totals[0], seeds: totals[1], born: totals[2], rounds,
                     deaths: all_deaths })
    }

    /// A 128-bit number summing up every tree and seed in the world:
    /// each row's values are scrambled into two 64-bit numbers, which are
    /// added up over all the rows (and all the ranks). Adding doesn't care
    /// about order, so it's the same however the rows are shared out, and
    /// any difference anywhere changes it. The same as world.py's.
    pub fn fingerprint(&self, comm: &mut dyn Comm) -> Result<String, Problem> {
        let forest = &self.forest;
        let (first, second) = (0..forest.len())
            .into_par_iter()
            .map(|row| row_fingerprint(forest, row))
            .reduce(|| (0, 0), |a, b| (a.0.wrapping_add(b.0), a.1.wrapping_add(b.1)));
        let totals = comm.sum(&[first, second, forest.len() as u64])?;
        Ok(format!("{}:{:016x}{:016x}", totals[2], totals[0], totals[1]))
    }
}

// ---------------------------------------------------------------------
// Seeds between ranks
// ---------------------------------------------------------------------

/// A letter with these new seeds (a column at a time)
fn seed_letter(seeds: &[NewSeed], ids: &[u64], places: &[usize], cycle: u32) -> Vec<u8> {
    let mut letter = Vec::with_capacity(12 + places.len() * 48);
    (places.len() as u64).write(&mut letter);
    (cycle as i32).write(&mut letter);
    write_column(&mut letter, ids, places);
    for &place in places {
        seeds[place].species.write(&mut letter);
    }
    for &place in places {
        seeds[place].x.write(&mut letter);
    }
    for &place in places {
        seeds[place].y.write(&mut letter);
    }
    for &place in places {
        seeds[place].count_to_germ.write(&mut letter);
    }
    for &place in places {
        seeds[place].mass_seed.write(&mut letter);
    }
    for &place in places {
        seeds[place].radius_seed.write(&mut letter);
    }
    letter
}

/// The seeds in a letter, added to the end of the forest
fn read_seed_letter(letter: &[u8], forest: &mut Forest) -> Result<(), Problem> {
    let mut reader = Reader::new(letter);
    let count = reader.value::<u64>() as usize;
    let cycle: i32 = reader.value();
    let first = forest.len();
    reader.column(count, &mut forest.id);
    reader.column(count, &mut forest.species);
    reader.column(count, &mut forest.x);
    reader.column(count, &mut forest.y);
    reader.column(count, &mut forest.count_to_germ);
    reader.column(count, &mut forest.mass_seed);
    reader.column(count, &mut forest.radius_seed);
    if !reader.finished() {
        return Err("a letter of seeds was the wrong length".into());
    }
    // everything else about a new seed starts at zero
    forest.fill_to(first + count);
    for row in first..first + count {
        forest.is_seed[row] = true;
        forest.birth_cycle[row] = cycle;
        forest.r[row] = forest.radius_seed[row];
        forest.mass_total[row] = forest.mass_seed[row];
    }
    Ok(())
}
