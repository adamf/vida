// Germinating, growing, making and throwing seeds, the deaths each tree
// decides for itself, and photosynthesis: the same sums, in the same order,
// as worldscale/forest.py (which follows Vida's vplantr.py and vworldr.py).
// Each function goes through the trees one at a time; nothing one tree does
// changes another, so the order doesn't matter.

use crate::maths;
use crate::philox;
use crate::settings::{Species, World};

/// How many cycles of growth a plant remembers, at most (forest.MEMORY_SLOTS)
pub const MEMORY_SLOTS: usize = 4;

/// Every column of the table of trees and seeds (see forest.COLUMNS)
pub struct Trees<'a> {
    pub id: &'a mut [u64],
    pub species: &'a mut [i32],
    pub is_seed: &'a mut [bool],
    pub x: &'a mut [f64],
    pub y: &'a mut [f64],
    #[allow(dead_code)]
    pub birth_cycle: &'a mut [i32],
    pub age: &'a mut [i32],
    pub count_to_germ: &'a mut [i32],
    pub mass_seed: &'a mut [f64],
    pub radius_seed: &'a mut [f64],
    pub mass_stem: &'a mut [f64],
    pub mass_leaf: &'a mut [f64],
    pub mass_fixed: &'a mut [f64],
    pub mass_total: &'a mut [f64],
    pub radius_stem: &'a mut [f64],
    pub radius_leaf: &'a mut [f64],
    pub r: &'a mut [f64],
    pub height_stem: &'a mut [f64],
    pub is_mature: &'a mut [bool],
    pub area_covered: &'a mut [f64],
    pub fixed_count: &'a mut [i32],
    pub height_count: &'a mut [i32],
    pub prev_height: &'a mut [f64],
    pub avg_height_growth: &'a mut [f64],
    pub max_avg_height_growth: &'a mut [f64],
    pub attached_count: &'a mut [i32],
    pub attached_mass: &'a mut [f64],
    /// MEMORY_SLOTS values per tree, the newest last
    pub fixed_record: &'a mut [f64],
    pub height_record: &'a mut [f64],
}

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
// Germinating and growing
// ---------------------------------------------------------------------

/// How many died of what (the order of forest.CAUSES)
#[derive(Default)]
pub struct Deaths {
    pub failed_random: i64,
    pub failed_immature: i64,
    pub impossible_height: i64,
    pub stem_off_world: i64,
    pub random_death: i64,
    pub growth_too_slow: i64,
    pub buckled: i64,
    pub lack_of_light: i64,
}

/// Vida's germinate() for every seed. Sets dying[row] for the seeds that die.
pub fn germinate(trees: &mut Trees, cycle: u32, table: &Species, world: &World, rng_start: u64,
                 dying: &mut [bool], deaths: &mut Deaths) {
    for row in 0..trees.id.len() {
        if !trees.is_seed[row] {
            continue;
        }
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
            continue;
        }
        if failed {
            deaths.failed_random += 1;
            dying[row] = true;
            continue;
        }
        if too_small {
            deaths.failed_immature += 1;
            dying[row] = true;
            continue;
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
            deaths.impossible_height += 1;
            dying[row] = true;
        }
    }
}

/// A plant whose seeds are ready to be thrown: which row, how many seeds,
/// and the mass of each
pub struct Dispersing {
    pub row: usize,
    pub count: i64,
    pub mass: f64,
}

/// Vida's growPlant() for these rows (the plants at the start of the cycle):
/// feed the seeds growing on the plant, start new ones, then grow the stem
/// and leaves.
pub fn grow(trees: &mut Trees, plants: &[i64], table: &Species, world: &World,
            dying: &mut [bool], deaths: &mut Deaths) -> Vec<Dispersing> {
    let mut dispersing = Vec::new();
    for &plant in plants {
        let row = plant as usize;
        let s = trees.species[row] as usize;
        let makes = table.makes_seeds[s];
        let mut fixed = trees.mass_fixed[row];

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
            dispersing.push(Dispersing { row, count, mass: attached_mass });
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
            deaths.impossible_height += 1;
            dying[row] = true;
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
    }
    dispersing
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

/// Where each dispersing seed lands (Vida's makeSeed for where on the canopy
/// it formed, then disperseSeed)
pub fn disperse(trees: &Trees, dispersing: &[Dispersing], cycle: u32, table: &Species, world: &World,
                rng_start: u64) -> Vec<NewSeed> {
    let mut seeds = Vec::new();
    for mother in dispersing {
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
    }
    seeds
}

// ---------------------------------------------------------------------
// Deaths each tree decides for itself
// ---------------------------------------------------------------------

/// Stems off the world, random death, growing too slowly and buckling, in
/// the order Vida checks them
pub fn own_deaths(trees: &Trees, cycle: u32, table: &Species, world: &World, rng_start: u64,
                  dying: &mut [bool], deaths: &mut Deaths) {
    let half = world.world_size / 2.0;
    // the part of the buckling height that only depends on the species
    let mut stiffness = Vec::with_capacity(table.density_stem.len());
    for s in 0..table.density_stem.len() {
        let youngs = table.youngs_modulus_stem[s] * 1_000_000_000.0;
        stiffness.push(maths::pow(youngs / (world.gravity * table.density_stem[s]), 0.3333));
    }
    for row in 0..trees.id.len() {
        if dying[row] {
            // already dying (from germinating or growing this cycle)
            continue;
        }
        let plant = !trees.is_seed[row];
        let s = trees.species[row] as usize;
        if !world.allow_off_world {
            let radius = if plant { trees.radius_stem[row] } else { trees.radius_seed[row] };
            let x = trees.x[row];
            let y = trees.y[row];
            if x + radius > half || x - radius < -half || y + radius > half || y - radius < -half {
                deaths.stem_off_world += 1;
                dying[row] = true;
                continue;
            }
        }
        if world.allow_random_death {
            let too_bad = philox::random_block(rng_start, trees.id[row], cycle, philox::RANDOM_DEATH, 0)[0];
            let chance = if plant { world.random_death_plant } else { world.random_death_seed };
            if too_bad < chance {
                deaths.random_death += 1;
                dying[row] = true;
                continue;
            }
        }
        if world.allow_slow_growth_death && plant && trees.max_avg_height_growth[row] > 0.0 {
            let fraction = trees.avg_height_growth[row] / trees.max_avg_height_growth[row];
            if table.random_slow_growth[s] > fraction || world.random_slow_growth > fraction {
                let too_bad = philox::random_block(rng_start, trees.id[row], cycle, philox::SLOW_GROWTH, 0)[0];
                if too_bad <= world.random_death_plant {
                    deaths.growth_too_slow += 1;
                    dying[row] = true;
                    continue;
                }
            }
        }
        if !world.allow_euler_greenhill_violations && plant {
            let critical = 0.79 * stiffness[s] * maths::pow(trees.radius_stem[row] * 2.0, 0.6667);
            if trees.height_stem[row] >= critical {
                deaths.buckled += 1;
                dying[row] = true;
            }
        }
    }
}

// ---------------------------------------------------------------------
// Photosynthesis
// ---------------------------------------------------------------------

/// calcNewMassFromLeaf for every plant
pub fn photosynthesise(trees: &mut Trees, table: &Species, dying: &mut [bool], deaths: &mut Deaths) {
    for row in 0..trees.id.len() {
        if trees.is_seed[row] {
            continue;
        }
        let s = trees.species[row] as usize;
        let area_photosynthesis = 3.14 * trees.radius_leaf[row] * trees.radius_leaf[row];
        let available = area_photosynthesis - trees.area_covered[row];
        let fraction = if area_photosynthesis > 0.0 { available / area_photosynthesis } else { 0.0 };
        if !(fraction > table.fraction_minimum_survival[s]) {
            deaths.lack_of_light += 1;
            dying[row] = true;
            continue;
        }
        let per_leaf = maths::pow(trees.mass_leaf[row], table.photo_exponent[s]);
        let rate = table.photo_constant[s] * fraction + table.photo_constant_shade[s] * (1.0 - fraction);
        let new_mass = rate * available * per_leaf;
        trees.mass_fixed[row] = new_mass;
        let start = row * MEMORY_SLOTS;
        push_record(&mut trees.fixed_record[start..start + MEMORY_SLOTS], &mut trees.fixed_count[row], new_mass);
    }
}
