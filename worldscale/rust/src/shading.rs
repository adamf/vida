// Vida's classic shading (determineShade), as worldscale/forest.py's shade
// does it: a plant is shaded by the taller canopies it overlaps. No
// canopies: all its area gets light. One: the overlapping area, less what
// gets through. Two or more: photons are dropped on the plant, and each stops
// at the first (tallest) canopy it lands in, getting through that one with
// the canopy's transmittance. Each photon's random numbers have their own
// address (the plant, the cycle and the photon's number).

use crate::maths;
use std::cmp::Ordering;

use crate::pairs;
use crate::philox;
use crate::settings::{Species, World};

/// Taller first; for the same height, the lower id first
fn compare_taller(a: &(f64, u64, usize), b: &(f64, u64, usize)) -> Ordering {
    match b.0.partial_cmp(&a.0) {
        Some(Ordering::Less) => Ordering::Less,
        Some(Ordering::Greater) => Ordering::Greater,
        _ => a.1.cmp(&b.1),
    }
}

/// geometry_utils.areaOverlappingCircles
pub fn lens_area(x: f64, y: f64, r: f64, xx: f64, yy: f64, rr: f64) -> f64 {
    let distance = maths::hypot(x - xx, y - yy);
    if distance < (r - rr).abs() {
        return 3.14 * r * r;
    }
    if !(distance <= r + rr) || !(distance > 0.0) {
        return 0.0;
    }
    let d = distance;
    let d0 = (r * r - rr * rr + d * d) / (2.0 * d);
    let d1 = (rr * rr - r * r + d * d) / (2.0 * d);
    let squared = r * r - d0 * d0;
    let half_line = (if squared > 0.0 { squared } else { 0.0 }).sqrt();
    let angle0 = maths::acos((d0 / r).clamp(-1.0, 1.0)) * 2.0;
    let angle1 = maths::acos((d1 / rr).clamp(-1.0, 1.0)) * 2.0;
    let sector0 = 0.5 * r * r * angle0;
    let sector1 = 0.5 * rr * rr * angle1;
    let triangle0 = 2.0 * 0.5 * d0 * half_line;
    let triangle1 = 2.0 * 0.5 * d1 * half_line;
    (sector0 - triangle0) + (sector1 - triangle1)
}

/// The shaded area of each of the first `owned` rows. The rest are the
/// neighbours' plants near enough to shade them.
#[allow(clippy::too_many_arguments)]
pub fn shade(owned: usize, x: &[f64], y: &[f64], r: &[f64], is_plant: &[bool], height: &[f64],
             ids: &[u64], species: &[i32], cycle: u32, table: &Species, world: &World,
             photon_limit: i64, rng_start: u64) -> Vec<f64> {
    let count = x.len();

    // pairs of overlapping plant canopies
    let mut plants = Vec::new();
    for row in 0..count {
        if is_plant[row] {
            plants.push(row);
        }
    }
    let mut plant_x = Vec::with_capacity(plants.len());
    let mut plant_y = Vec::with_capacity(plants.len());
    let mut plant_r = Vec::with_capacity(plants.len());
    for &row in &plants {
        plant_x.push(x[row]);
        plant_y.push(y[row]);
        plant_r.push(r[row]);
    }
    let (first, second) = pairs::find_pairs(&plant_x, &plant_y, &plant_r);

    // everyone's place, tallest first
    let mut order = Vec::with_capacity(count);
    for row in 0..count {
        order.push((height[row], ids[row], row));
    }
    order.sort_by(compare_taller);
    let mut place = vec![0usize; count];
    for (position, entry) in order.iter().enumerate() {
        place[entry.2] = position;
    }

    // who covers whom: the taller one covers the other. Only our own
    // plants' shade is ours to work out.
    let mut covering: Vec<(usize, usize, usize)> = Vec::new();
    for pair in 0..first.len() {
        let a = plants[first[pair] as usize];
        let b = plants[second[pair] as usize];
        let (shaded, cover) = if place[a] < place[b] { (b, a) } else { (a, b) };
        if shaded < owned {
            covering.push((shaded, place[cover], cover));
        }
    }
    // each plant's covers, tallest first
    covering.sort_unstable();
    let mut cover_count = vec![0usize; owned];
    let mut first_cover = vec![0usize; owned];
    for (position, entry) in covering.iter().enumerate() {
        if cover_count[entry.0] == 0 {
            first_cover[entry.0] = position;
        }
        cover_count[entry.0] += 1;
    }

    let light = world.light_intensity;
    let mut covered = vec![0.0; owned];
    for row in 0..owned {
        let radius = r[row];
        let area_total = 3.14 * radius * radius;
        let mut exposed = light;
        if is_plant[row] && cover_count[row] == 1 {
            // one cover: the overlapping area, worked out exactly
            let other = covering[first_cover[row]].2;
            let mut area = lens_area(x[row], y[row], radius, x[other], y[other], r[other]);
            area = area - area * table.canopy_transmittance[species[other] as usize];
            exposed = if area_total > 0.0 { (area_total - area) / area_total } else { 1.0 } * light;
        } else if is_plant[row] && cover_count[row] >= 2 {
            // two or more: photons
            let mut photons = (area_total as i64).max(1) * 100;
            if photons > photon_limit {
                photons = photon_limit;
            }
            let covers = &covering[first_cover[row]..first_cover[row] + cover_count[row]];
            let mut hits = 0i64;
            for number in 0..photons {
                let block = philox::random_block(rng_start, ids[row], cycle, philox::PHOTON, number as u32);
                // a point spread evenly over the plant's circle
                let distance = radius * block[0].sqrt();
                let angle = block[1] * (2.0 * std::f64::consts::PI);
                let (sine, cosine) = maths::sincos(angle);
                let photon_x = x[row] + distance * cosine;
                let photon_y = y[row] + distance * sine;
                let mut blocked = false;
                for entry in covers {
                    let canopy = entry.2;
                    // inside the canopy? (comparing squared distances: exact
                    // arithmetic, so the same on every computer; forest.py
                    // uses hypot, which can only differ right on the edge)
                    let dx = x[canopy] - photon_x;
                    let dy = y[canopy] - photon_y;
                    if dx * dx + dy * dy <= r[canopy] * r[canopy] {
                        blocked = block[2] > table.canopy_transmittance[species[canopy] as usize];
                        break;
                    }
                }
                if !blocked {
                    hits += 1;
                }
            }
            exposed = (hits as f64) / (photons as f64) * light;
        }
        covered[row] = area_total - area_total * exposed;
    }
    covered
}
