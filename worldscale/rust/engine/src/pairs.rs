// Finding every pair of circles that overlap (or touch), each pair once: the
// same pairs worldscale/forest.py's findPairs finds.
//
// Circles come in all sizes, from seeds a few centimetres across to
// canopies ten metres across, so each pair is found by the larger circle of
// the two: a circle only looks for circles no bigger than itself, so it
// only needs to look as far as twice its own radius.
//
// The circles are filed into a grid of squares (a counting sort: count how
// many are in each square, then drop each into its place). The squares are
// about the size of the larger circles, or big enough to hold about one
// circle each on average, whichever is bigger. A circle then looks in the
// squares within twice its radius of its own.
//
// The circles are shared out between the cores in runs of the grid, and the
// pairs each run finds are put together in grid order, so the pairs come out
// in the same order however many cores there are.

use rayon::prelude::*;

/// How many circles each core takes at a time
const RUN_LENGTH: usize = 4096;

/// About the value that `share` of the (non-negative) values are no bigger
/// than: exactly that, of an evenly spaced sample of at most 65,536 of them.
/// (It only sets the size of the grid's squares, which doesn't change the
/// pairs found.)
fn quantile(values: &[f64], share: f64) -> f64 {
    // non-negative numbers sort the same way as their bits
    let step = values.len().div_ceil(65_536).max(1);
    let mut bits: Vec<u64> = values.iter().step_by(step).map(|&value| if value > 0.0 { value.to_bits() } else { 0 }).collect();
    let place = ((bits.len() - 1) as f64 * share) as usize;
    let (_, middle, _) = bits.select_nth_unstable(place);
    f64::from_bits(*middle)
}

/// The smallest and largest of some numbers
fn bounds(values: &[f64]) -> (f64, f64) {
    values
        .par_chunks(RUN_LENGTH)
        .map(|run| run.iter().fold((values[0], values[0]), |(low, high), &value| (low.min(value), high.max(value))))
        .reduce(|| (values[0], values[0]), |a, b| (a.0.min(b.0), a.1.max(b.1)))
}

pub fn find_pairs(x: &[f64], y: &[f64], radius: &[f64]) -> (Vec<i64>, Vec<i64>) {
    let mut first = Vec::new();
    let mut second = Vec::new();
    let count = x.len();
    if count < 2 {
        return (first, second);
    }

    // the grid
    let (low_x, high_x) = bounds(x);
    let (low_y, high_y) = bounds(y);
    let area = ((high_x - low_x) * (high_y - low_y)).max(1e-12);
    let square = (2.0 * quantile(radius, 0.9)).max((area / count as f64).sqrt()).max(1e-9);
    let columns = ((high_x - low_x) / square) as usize + 1;
    let rows = ((high_y - low_y) / square) as usize + 1;

    // file every circle under its square (a counting sort)
    let square_of: Vec<usize> = (0..count)
        .into_par_iter()
        .with_min_len(RUN_LENGTH)
        .map(|row| {
            let column = (((x[row] - low_x) / square) as usize).min(columns - 1);
            let row_of_squares = (((y[row] - low_y) / square) as usize).min(rows - 1);
            column * rows + row_of_squares
        })
        .collect();
    let mut in_square = vec![0usize; columns * rows + 1];
    for &place in &square_of {
        in_square[place + 1] += 1;
    }
    for place in 1..in_square.len() {
        in_square[place] += in_square[place - 1];
    }
    let mut filed = vec![0usize; count];
    let mut next = in_square.clone();
    for row in 0..count {
        filed[next[square_of[row]]] = row;
        next[square_of[row]] += 1;
    }
    // copies of the positions and sizes in grid order, so the circles
    // compared are next to each other in memory
    let grid_x: Vec<f64> = filed.par_iter().with_min_len(RUN_LENGTH).map(|&row| x[row]).collect();
    let grid_y: Vec<f64> = filed.par_iter().with_min_len(RUN_LENGTH).map(|&row| y[row]).collect();
    let grid_radius: Vec<f64> = filed.par_iter().with_min_len(RUN_LENGTH).map(|&row| radius[row]).collect();

    // each circle, in grid order, looks for circles no bigger than itself
    let look = |run: usize| -> (Vec<i64>, Vec<i64>) {
        let mut first = Vec::new();
        let mut second = Vec::new();
        for place in run * RUN_LENGTH..((run + 1) * RUN_LENGTH).min(count) {
            let query = filed[place];
            let query_x = grid_x[place];
            let query_y = grid_y[place];
            let query_radius = grid_radius[place];
            let reach_squares = ((2.0 * query_radius / square).ceil() as i64).max(1);
            let column = (square_of[query] / rows) as i64;
            let row_of_squares = (square_of[query] % rows) as i64;
            let first_column = (column - reach_squares).max(0);
            let last_column = (column + reach_squares).min(columns as i64 - 1);
            let first_row = (row_of_squares - reach_squares).max(0);
            let last_row = (row_of_squares + reach_squares).min(rows as i64 - 1);
            for other_column in first_column..=last_column {
                let start = in_square[other_column as usize * rows + first_row as usize];
                let end = in_square[other_column as usize * rows + last_row as usize + 1];
                for other_place in start..end {
                    let other_radius = grid_radius[other_place];
                    // each pair once, found by the larger circle (or, for two the
                    // same size, the one with the lower row number)
                    if other_radius > query_radius {
                        continue;
                    }
                    let other = filed[other_place];
                    if other == query || (other_radius == query_radius && other < query) {
                        continue;
                    }
                    let dx = query_x - grid_x[other_place];
                    let dy = query_y - grid_y[other_place];
                    let reach = query_radius + other_radius;
                    if dx * dx + dy * dy <= reach * reach {
                        first.push(query as i64);
                        second.push(other as i64);
                    }
                }
            }
        }
        (first, second)
    };
    let runs: Vec<(Vec<i64>, Vec<i64>)> = (0..count.div_ceil(RUN_LENGTH)).into_par_iter().map(look).collect();
    for (run_first, run_second) in runs {
        first.extend(run_first);
        second.extend(run_second);
    }
    (first, second)
}

/// Is circle a stronger than circle b in an overlap? Vida's removeOverlaps:
/// the heavier wins; if they weigh the same, the one planted first; and then
/// the lower id (forest.strongerFirst's order)
fn stronger(a: usize, b: usize, mass: &[f64], birth: &[i32], ids: &[u64]) -> bool {
    if mass[a] > mass[b] {
        return true;
    }
    if mass[a] < mass[b] {
        return false;
    }
    if birth[a] != birth[b] {
        return birth[a] < birth[b];
    }
    ids[a] < ids[b]
}

/// Every pair of overlapping circles where the weaker one is among the first
/// `owned` rows: (the stronger's rows, the weaker's)
pub fn overlap_winners(x: &[f64], y: &[f64], radius: &[f64], mass: &[f64], birth: &[i32], ids: &[u64],
                       owned: usize) -> (Vec<i64>, Vec<i64>) {
    let (first, second) = find_pairs(x, y, radius);
    (0..first.len())
        .into_par_iter()
        .with_min_len(RUN_LENGTH)
        .filter_map(|pair| {
            let a = first[pair] as usize;
            let b = second[pair] as usize;
            let (winner, loser) = if stronger(a, b, mass, birth, ids) { (a, b) } else { (b, a) };
            if loser < owned {
                Some((winner as i64, loser as i64))
            } else {
                None
            }
        })
        .unzip()
}
