// Germinating, growing, making and throwing seeds, the deaths each tree
// decides for itself, and photosynthesis: the same sums, in the same order,
// as worldscale/forest.py (which follows Vida's vplantr.py and vworldr.py).
//
// Each sum is written for one tree (one row), and nothing one tree does
// changes another, so the order doesn't matter: the table is cut into
// pieces, and the cores of the computer each take pieces (with rayon). What
// the pieces give back is put together in row order, so the answers are the
// same however many cores there are.

use rayon::prelude::*;

use crate::forest::{Trees, MEMORY_SLOTS};
use crate::maths;
use crate::philox;
use crate::settings::{Species, World};

/// How many rows each piece of the table has
const PIECE_ROWS: usize = 2048;

/// numpy.maximum for two numbers
fn larger(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a >= b {
        a
    } else {
        b
    }
}

/// numpy.minimum for two numbers
fn smaller(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a <= b {
        a
    } else {
        b
    }
}

// ---------------------------------------------------------------------
// Sizes from masses (Vida's allometry)
// ---------------------------------------------------------------------

/// growSeedOnPlant: the radius of a ball of the seed's volume
pub fn seed_radius(mass_seed: f64, density_seed: f64) -> f64 {
    let volume = mass_seed / density_seed;
    maths::pow(volume / 1.3333 / 3.14, 0.3333)
}

/// calcRadiusStemFromMassStem
fn stem_radius(mass_stem: f64, s: usize, table: &Species) -> f64 {
    let diameter = table.constant20[s] * maths::pow(mass_stem, table.exponent20[s]);
    diameter / 2.0
}

/// calcHeightStemFromRadiusStem: the height for a stem this thick, and
/// whether the plant is now mature
fn stem_height(radius_stem: f64, is_mature: bool, s: usize, table: &Species) -> (f64, bool) {
    let diameter = radius_stem * 2.0;
    let grown = table.constant8[s] * maths::log(diameter) + table.height_stem_max[s];
    let young = table.constant7[s] * maths::pow(diameter, table.exponent7[s]) - table.constant6[s];
    let now_mature = is_mature || grown >= young;
    if now_mature {
        (grown, true)
    } else {
        (young, false)
    }
}

/// calcMassLeafFromMassStem
fn leaf_mass(mass_stem: f64, age: i32, is_mature: bool, s: usize, table: &Species) -> f64 {
    let older = (age as f64) >= table.start_making_seeds_age[s] || is_mature;
    if older {
        table.constant3[s] * maths::pow(mass_stem, table.exponent3[s])
    } else {
        table.constant2[s] * maths::pow(mass_stem, table.exponent2[s])
    }
}

/// calcRadiusLeafFromMassLeaf: leaves as a disc heightLeafMax thick
fn leaf_radius(mass_leaf: f64, s: usize, table: &Species) -> f64 {
    let volume = mass_leaf / table.density_leaf[s];
    let area = volume / table.height_leaf_max[s];
    let radius = (area / 3.14).sqrt();
    if table.leaf_is_hemisphere[s] {
        radius * 0.7071067812
    } else {
        radius
    }
}

/// The average of the last min(count, memory) values of a record (the
/// newest is in the last slot)
fn recent_average(record: &[f64], count: i32, memory: i64) -> f64 {
    let used = (count as i64).min(memory);
    let mut total = 0.0;
    for slot in 0..MEMORY_SLOTS {
        let age = (MEMORY_SLOTS - 1 - slot) as i64;
        if age < used {
            total += record[slot];
        } else {
            total += 0.0;
        }
    }
    if used > 0 {
        total / (used.max(1) as f64)
    } else {
        0.0
    }
}

/// Add this cycle's value to the end of a record
fn push_record(record: &mut [f64], count: &mut i32, value: f64) {
    for slot in 0..MEMORY_SLOTS - 1 {
        record[slot] = record[slot + 1];
    }
    record[MEMORY_SLOTS - 1] = value;
    *count = (*count + 1).min(MEMORY_SLOTS as i32);
}

// ---------------------------------------------------------------------
// Deaths, and cutting the table into pieces
// ---------------------------------------------------------------------

/// How many died of what, in the order of forest.CAUSES
#[derive(Default, Clone, Copy)]
pub struct Deaths {
    pub counts: [u64; 10],
}

pub const FAILED_RANDOM: usize = 0;
pub const FAILED_IMMATURE: usize = 1;
pub const IMPOSSIBLE_HEIGHT: usize = 2;
pub const STEM_OFF_WORLD: usize = 3;
pub const RANDOM_DEATH: usize = 4;
pub const GROWTH_TOO_SLOW: usize = 5;
pub const BUCKLED: usize = 6;
pub const CRUSHED: usize = 7;
pub const LACK_OF_LIGHT: usize = 8;
pub const LANDED_OFF_WORLD: usize = 9;

/// The causes of death, as forest.CAUSES names them
pub const CAUSES: [&str; 10] = [
    "failed to germinate(random death)",
    "failed to germinate(immaturity)",
    "impossible height calculation",
    "stem off world",
    "random death",
    "growth too slow",
    "violated Euler-Greenhill",
    "crushed",
    "lack of light",
    "seed landed off world",
];

impl std::ops::Add for Deaths {
    type Output = Deaths;

    fn add(mut self, other: Deaths) -> Deaths {
        for cause in 0..self.counts.len() {
            self.counts[cause] += other.counts[cause];
        }
        self
    }
}

/// A plant whose seeds are ready to be thrown: which row, how many seeds,
/// and the mass of each
pub struct Dispersing {
    pub row: usize,
    pub count: i64,
    pub mass: f64,
}

/// What a piece of the table gives back: the deaths counted, and the plants
/// throwing seeds
#[derive(Default)]
struct PieceResult {
    deaths: Deaths,
    dispersing: Vec<Dispersing>,
}

/// Cut the table into pieces and do `work(piece, first row, dying, result)`
/// to each, on all the cores at once. Gives back the deaths, added up, and
/// the plants throwing seeds, in row order.
fn in_pieces<F>(trees: &mut Trees, dying: &mut [bool], work: F) -> (Deaths, Vec<Dispersing>)
where
    F: Fn(&mut Trees, usize, &mut [bool], &mut PieceResult) + Sync,
{
    let mut pieces = Vec::new();
    let mut rest = trees.reborrow();
    let mut rest_dying = dying;
    let mut first = 0;
    while !rest.is_empty() {
        let size = PIECE_ROWS.min(rest.len());
        let (piece, after) = rest.split_at(size);
        let (piece_dying, after_dying) = rest_dying.split_at_mut(size);
        pieces.push((first, piece, piece_dying));
        rest = after;
        rest_dying = after_dying;
        first += size;
    }
    let results: Vec<PieceResult> = pieces
        .into_par_iter()
        .map(|(first, mut piece, piece_dying)| {
            let mut result = PieceResult::default();
            work(&mut piece, first, piece_dying, &mut result);
            result
        })
        .collect();
    let mut deaths = Deaths::default();
    let mut dispersing = Vec::new();
    for result in results {
        deaths = deaths + result.deaths;
        dispersing.extend(result.dispersing);
    }
    (deaths, dispersing)
}

// ---------------------------------------------------------------------
// Germinating and growing
// ---------------------------------------------------------------------

/// Vida's germinate() for one seed
#[allow(clippy::too_many_arguments)]
fn germinate_seed(trees: &mut Trees, row: usize, cycle: u32, table: &Species, world: &World, rng_start: u64,
                  dying: &mut bool, deaths: &mut Deaths) {
    let s = trees.species[row] as usize;
    let ready = trees.count_to_germ[row] < 1;
    let mut too_bad = philox::random_block(rng_start, trees.id[row], cycle, philox::GERMINATE, 0)[0];
    if cycle == 0 && world.ignore_germ_death_at_start {
        too_bad = 1.0;
    }
    let failed = ready && too_bad < table.fraction_fail_germinate[s];
    let mass_for_growth = trees.mass_seed[row] * table.fraction_seed_mass_to_plant[s];
    let too_small = mass_for_growth
        <= table.mass_seed_max[s] * table.fract_mass_seed_max_to_germ[s] * table.fraction_seed_mass_to_plant[s]
        || mass_for_growth <= 0.0;
    if !ready {
        trees.count_to_germ[row] -= 1;
        return;
    }
    if failed {
        deaths.counts[FAILED_RANDOM] += 1;
        *dying = true;
        return;
    }
    if too_small {
        deaths.counts[FAILED_IMMATURE] += 1;
        *dying = true;
        return;
    }
    // the seed becomes a plant
    trees.is_seed[row] = false;
    trees.age[row] = 1;
    trees.mass_stem[row] = mass_for_growth * table.fraction_carbon_to_stem[s];
    trees.mass_leaf[row] = mass_for_growth - trees.mass_stem[row];
    trees.radius_stem[row] = stem_radius(trees.mass_stem[row], s, table);
    let (height, mature) = stem_height(trees.radius_stem[row], trees.is_mature[row], s, table);
    trees.height_stem[row] = height;
    trees.is_mature[row] = mature;
    trees.radius_leaf[row] = leaf_radius(trees.mass_leaf[row], s, table);
    trees.r[row] = larger(trees.radius_leaf[row], trees.radius_stem[row]);
    trees.mass_total[row] = trees.mass_stem[row] + trees.mass_leaf[row];
    trees.mass_fixed[row] = 0.0;
    if height < 0.0 {
        deaths.counts[IMPOSSIBLE_HEIGHT] += 1;
        *dying = true;
    }
}

/// Vida's growPlant() for one plant: feed the seeds growing on the plant,
/// start new ones, then grow the stem and leaves. Gives back the seeds to
/// throw, if they're ready.
fn grow_plant(trees: &mut Trees, row: usize, table: &Species, world: &World, dying: &mut bool,
              deaths: &mut Deaths) -> Option<(i64, f64)> {
    let s = trees.species[row] as usize;
    let makes = table.makes_seeds[s];
    let mut fixed = trees.mass_fixed[row];
    let mut thrown = None;

    // 1. seeds on the plant get their share of the carbon, as one cohort
    let mut count = trees.attached_count[row] as i64;
    let has_seeds = makes && count > 0;
    let mass_seed_max = table.mass_seed_max[s];
    let mut share = if has_seeds {
        fixed * table.fraction_carbon_to_seeds[s] / (count.max(1) as f64)
    } else {
        0.0
    };
    share = smaller(share, mass_seed_max);
    if !(fixed > 0.0) {
        share = 0.0;
    }
    let total = share * (count as f64);
    if total > fixed && count > 0 {
        share = fixed / (count.max(1) as f64);
    }
    fixed -= share * (count as f64);
    let mut attached_mass = trees.attached_mass[row] + share;
    if has_seeds && attached_mass >= mass_seed_max {
        thrown = Some((count, attached_mass));
        count = 0;
        attached_mass = 0.0;
    }

    // 2. mature plants start new seeds, when none are growing on them
    let start = row * MEMORY_SLOTS;
    let average_fixed = recent_average(&trees.fixed_record[start..start + MEMORY_SLOTS],
                                       trees.fixed_count[row], table.memory[s]);
    let max_kg_seeds = table.reproduction_constant[s] * maths::pow(larger(fixed, 0.0), table.reproduction_exponent[s]);
    let mut adjusted = max_kg_seeds * table.fraction_carbon_to_seeds[s];
    let average_or_one = if average_fixed > 0.0 { average_fixed } else { 1.0 };
    let stressed = fixed < average_fixed
        && fixed / average_or_one < table.fraction_selfishness[s]
        && table.fraction_carbon_to_seeds[s] < 1.0;
    if stressed {
        adjusted = max_kg_seeds;
    }
    let mut new_seeds = (adjusted / mass_seed_max) as i64 - count;
    new_seeds = new_seeds.min(world.max_seeds_per_plant);
    if makes && trees.is_mature[row] && count == 0 && new_seeds > 0 {
        count = new_seeds;
        attached_mass = 0.0;
    }
    trees.attached_count[row] = count as i32;
    trees.attached_mass[row] = attached_mass;
    trees.mass_fixed[row] = fixed;

    // 3. the stem, then the leaves
    let mass_stem = trees.mass_stem[row] + table.constant1[s] * maths::pow(larger(fixed, 0.0), table.exponent1[s]);
    let radius_stem = stem_radius(mass_stem, s, table);
    let (height, mature) = stem_height(radius_stem, trees.is_mature[row], s, table);
    let mass_leaf = leaf_mass(mass_stem, trees.age[row], mature, s, table);
    let radius_leaf = leaf_radius(mass_leaf, s, table);
    trees.mass_stem[row] = mass_stem;
    trees.radius_stem[row] = radius_stem;
    trees.height_stem[row] = height;
    trees.is_mature[row] = mature;
    trees.mass_leaf[row] = mass_leaf;
    trees.radius_leaf[row] = radius_leaf;
    trees.mass_total[row] = mass_leaf + mass_stem + attached_mass * (count as f64);
    trees.r[row] = larger(radius_leaf, radius_stem);
    if radius_stem <= 0.0 || !(height >= 0.0) {
        deaths.counts[IMPOSSIBLE_HEIGHT] += 1;
        *dying = true;
    }

    // 4. remember how much the stem grew
    let growth = height - trees.prev_height[row];
    push_record(&mut trees.height_record[start..start + MEMORY_SLOTS], &mut trees.height_count[row], growth);
    trees.prev_height[row] = height;
    let average = recent_average(&trees.height_record[start..start + MEMORY_SLOTS],
                                 trees.height_count[row], table.memory[s]);
    trees.avg_height_growth[row] = average;
    trees.max_avg_height_growth[row] = larger(trees.max_avg_height_growth[row], average);
    trees.age[row] += 1;
    thrown
}

/// Vida's germinate() for every seed. Sets dying[row] for the seeds that die.
pub fn germinate(trees: &mut Trees, cycle: u32, table: &Species, world: &World, rng_start: u64,
                 dying: &mut [bool]) -> Deaths {
    let work = |piece: &mut Trees, _first: usize, dying: &mut [bool], result: &mut PieceResult| {
        for row in 0..piece.len() {
            if piece.is_seed[row] {
                germinate_seed(piece, row, cycle, table, world, rng_start, &mut dying[row], &mut result.deaths);
            }
        }
    };
    in_pieces(trees, dying, work).0
}

/// Vida's growPlant() for the rows marked in `plants` (the plants at the
/// start of the cycle). Gives back the deaths, and the plants throwing
/// seeds, in row order.
pub fn grow(trees: &mut Trees, plants: &[bool], table: &Species, world: &World,
            dying: &mut [bool]) -> (Deaths, Vec<Dispersing>) {
    let work = |piece: &mut Trees, first: usize, dying: &mut [bool], result: &mut PieceResult| {
        for row in 0..piece.len() {
            if plants[first + row] {
                if let Some((count, mass)) = grow_plant(piece, row, table, world, &mut dying[row], &mut result.deaths) {
                    result.dispersing.push(Dispersing { row: first + row, count, mass });
                }
            }
        }
    };
    in_pieces(trees, dying, work)
}

/// Both at once, in one pass through the table: seeds germinate, plants
/// grow. (The same as germinate() then grow() of the plants there were
/// before: the two never touch the same row.)
pub fn germinate_and_grow(trees: &mut Trees, cycle: u32, table: &Species, world: &World, rng_start: u64,
                          dying: &mut [bool]) -> (Deaths, Vec<Dispersing>) {
    let work = |piece: &mut Trees, first: usize, dying: &mut [bool], result: &mut PieceResult| {
        for row in 0..piece.len() {
            if piece.is_seed[row] {
                germinate_seed(piece, row, cycle, table, world, rng_start, &mut dying[row], &mut result.deaths);
            } else if let Some((count, mass)) = grow_plant(piece, row, table, world, &mut dying[row], &mut result.deaths) {
                result.dispersing.push(Dispersing { row: first + row, count, mass });
            }
        }
    };
    in_pieces(trees, dying, work)
}

/// A new seed on the ground
pub struct NewSeed {
    pub mother_id: u64,
    pub seed_number: i64,
    pub mother_x: f64,
    pub mother_y: f64,
    pub species: i32,
    pub x: f64,
    pub y: f64,
    pub mass_seed: f64,
    pub radius_seed: f64,
    pub count_to_germ: i32,
}

/// Where one plant's seeds land
fn throw_seeds(trees: &Trees, mother: &Dispersing, cycle: u32, table: &Species, world: &World,
               rng_start: u64) -> Vec<NewSeed> {
    let mut seeds = Vec::with_capacity(mother.count.max(0) as usize);
    let row = mother.row;
    let s = trees.species[row] as usize;
    let mother_id = trees.id[row];
    let mother_x = trees.x[row];
    let mother_y = trees.y[row];
    for number in 0..mother.count {
        let index = number as u32;
        // where on the canopy it formed
        let form = philox::random_block(rng_start, mother_id, cycle, philox::FORM_SEED, index);
        let canopy = trees.radius_leaf[row];
        let outer = canopy * table.formation_max[s];
        let inner = canopy * table.formation_min[s];
        let distance = form[0] * (outer - inner) + inner;
        let angle = form[1] * (3.14 * 2.0);
        let formed_x = distance * maths::cos(angle) + mother_x;
        let formed_y = distance * maths::sin(angle) + mother_y;

        // how it's thrown
        let throw = philox::random_block(rng_start, mother_id, cycle, philox::DISPERSE, index);
        let method = table.dispersal_method[s];
        let mut new_x = formed_x;
        let mut new_y = formed_y;
        if method == 0 {
            // anywhere in the world
            new_x = (throw[0] - 0.5) * world.world_size;
            new_y = (throw[1] - 0.5) * world.world_size;
        } else if method == 2 {
            // in a circle round where it formed
            let reach = throw[0] * table.dispersal1[s];
            let turn = throw[1] * (3.14 * 2.0);
            new_x = formed_x + reach * maths::cos(turn);
            new_y = formed_y + reach * maths::sin(turn);
        } else if method == 3 || method == 4 {
            // thrown outwards from the middle of the plant
            let mut throw_distance = throw[0] * table.dispersal1[s];
            if method == 4 {
                // ballistic, from the launch angle and speed, each varied by up to half
                let mut launch = table.dispersal1[s];
                let mut change = throw[0] * (launch * 0.5);
                if throw[1] > 0.5 {
                    change = -change;
                }
                launch = (launch + change) * (std::f64::consts::PI / 180.0);
                let mut speed = table.dispersal2[s];
                let mut speed_change = throw[2] * (speed * 0.5);
                if throw[3] > 0.5 {
                    speed_change = -speed_change;
                }
                speed += speed_change;
                let height = trees.height_stem[row] + table.height_leaf_max[s];
                let g = world.gravity;
                let across = (speed * maths::cos(launch)) / g;
                let up = speed * maths::sin(launch);
                throw_distance = across * (up + (up * up + 2.0 * g * height).sqrt());
            }
            let run = formed_x - mother_x;
            let rise = formed_y - mother_y;
            let hypot = maths::hypot(run, rise);
            let direction = if hypot > 0.0 {
                maths::asin((rise / hypot).clamp(-1.0, 1.0))
            } else {
                0.0
            };
            let mut along_x = maths::cos(direction) * throw_distance;
            if run < 0.0 {
                along_x = -along_x;
            }
            new_x = along_x + formed_x;
            new_y = maths::sin(direction) * throw_distance + formed_y;
        }
        seeds.push(NewSeed {
            mother_id,
            seed_number: number,
            mother_x,
            mother_y,
            species: s as i32,
            x: new_x,
            y: new_y,
            mass_seed: mother.mass,
            radius_seed: seed_radius(mother.mass, table.density_seed[s]),
            count_to_germ: table.delay_in_germination[s] as i32,
        });
    }
    seeds
}

/// Where each dispersing seed lands (Vida's makeSeed for where on the canopy
/// it formed, then disperseSeed), in the order of the plants
pub fn disperse(trees: &Trees, dispersing: &[Dispersing], cycle: u32, table: &Species, world: &World,
                rng_start: u64) -> Vec<NewSeed> {
    dispersing
        .par_iter()
        .with_min_len(64)
        .flat_map_iter(|mother| throw_seeds(trees, mother, cycle, table, world, rng_start))
        .collect()
}

// ---------------------------------------------------------------------
// Deaths each tree decides for itself
// ---------------------------------------------------------------------

/// Stems off the world, random death, growing too slowly and buckling, in
/// the order Vida checks them, for one tree not already dying
#[allow(clippy::too_many_arguments)]
fn own_death(trees: &Trees, row: usize, cycle: u32, table: &Species, world: &World, rng_start: u64,
             stiffness: &[f64], dying: &mut bool, deaths: &mut Deaths) {
    let half = world.world_size / 2.0;
    let plant = !trees.is_seed[row];
    let s = trees.species[row] as usize;
    if !world.allow_off_world {
        let radius = if plant { trees.radius_stem[row] } else { trees.radius_seed[row] };
        let x = trees.x[row];
        let y = trees.y[row];
        if x + radius > half || x - radius < -half || y + radius > half || y - radius < -half {
            deaths.counts[STEM_OFF_WORLD] += 1;
            *dying = true;
            return;
        }
    }
    if world.allow_random_death {
        let too_bad = philox::random_block(rng_start, trees.id[row], cycle, philox::RANDOM_DEATH, 0)[0];
        let chance = if plant { world.random_death_plant } else { world.random_death_seed };
        if too_bad < chance {
            deaths.counts[RANDOM_DEATH] += 1;
            *dying = true;
            return;
        }
    }
    if world.allow_slow_growth_death && plant && trees.max_avg_height_growth[row] > 0.0 {
        let fraction = trees.avg_height_growth[row] / trees.max_avg_height_growth[row];
        if table.random_slow_growth[s] > fraction || world.random_slow_growth > fraction {
            let too_bad = philox::random_block(rng_start, trees.id[row], cycle, philox::SLOW_GROWTH, 0)[0];
            if too_bad <= world.random_death_plant {
                deaths.counts[GROWTH_TOO_SLOW] += 1;
                *dying = true;
                return;
            }
        }
    }
    if !world.allow_euler_greenhill_violations && plant {
        let critical = 0.79 * stiffness[s] * maths::pow(trees.radius_stem[row] * 2.0, 0.6667);
        if trees.height_stem[row] >= critical {
            deaths.counts[BUCKLED] += 1;
            *dying = true;
        }
    }
}

/// The deaths each tree decides for itself, for the trees not already dying
pub fn own_deaths(trees: &Trees, cycle: u32, table: &Species, world: &World, rng_start: u64,
                  dying: &mut [bool]) -> Deaths {
    // the part of the buckling height that only depends on the species
    let mut stiffness = Vec::with_capacity(table.density_stem.len());
    for s in 0..table.density_stem.len() {
        let youngs = table.youngs_modulus_stem[s] * 1_000_000_000.0;
        stiffness.push(maths::pow(youngs / (world.gravity * table.density_stem[s]), 0.3333));
    }
    dying
        .par_chunks_mut(PIECE_ROWS)
        .enumerate()
        .map(|(piece, piece_dying)| {
            let mut deaths = Deaths::default();
            let first = piece * PIECE_ROWS;
            for place in 0..piece_dying.len() {
                if !piece_dying[place] {
                    own_death(trees, first + place, cycle, table, world, rng_start, &stiffness,
                              &mut piece_dying[place], &mut deaths);
                }
            }
            deaths
        })
        .reduce(Deaths::default, std::ops::Add::add)
}

// ---------------------------------------------------------------------
// Photosynthesis
// ---------------------------------------------------------------------

/// calcNewMassFromLeaf for one plant
fn photosynthesise_plant(trees: &mut Trees, row: usize, table: &Species, dying: &mut bool, deaths: &mut Deaths) {
    let s = trees.species[row] as usize;
    let area_photosynthesis = 3.14 * trees.radius_leaf[row] * trees.radius_leaf[row];
    let available = area_photosynthesis - trees.area_covered[row];
    let fraction = if area_photosynthesis > 0.0 { available / area_photosynthesis } else { 0.0 };
    if !(fraction > table.fraction_minimum_survival[s]) {
        deaths.counts[LACK_OF_LIGHT] += 1;
        *dying = true;
        return;
    }
    let per_leaf = maths::pow(trees.mass_leaf[row], table.photo_exponent[s]);
    let rate = table.photo_constant[s] * fraction + table.photo_constant_shade[s] * (1.0 - fraction);
    let new_mass = rate * available * per_leaf;
    trees.mass_fixed[row] = new_mass;
    let start = row * MEMORY_SLOTS;
    push_record(&mut trees.fixed_record[start..start + MEMORY_SLOTS], &mut trees.fixed_count[row], new_mass);
}

/// calcNewMassFromLeaf for every plant
pub fn photosynthesise(trees: &mut Trees, table: &Species, dying: &mut [bool]) -> Deaths {
    let work = |piece: &mut Trees, _first: usize, dying: &mut [bool], result: &mut PieceResult| {
        for row in 0..piece.len() {
            if !piece.is_seed[row] {
                photosynthesise_plant(piece, row, table, &mut dying[row], &mut result.deaths);
            }
        }
    };
    in_pieces(trees, dying, work).0
}
