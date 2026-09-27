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

/// The value that `share` of the (non-negative) values are no bigger than
fn quantile(values: &[f64], share: f64) -> f64 {
    // non-negative numbers sort the same way as their bits
    let mut bits = Vec::with_capacity(values.len());
    for &value in values {
        bits.push(if value > 0.0 { value.to_bits() } else { 0 });
    }
    let place = ((values.len() - 1) as f64 * share) as usize;
    let (_, middle, _) = bits.select_nth_unstable(place);
    f64::from_bits(*middle)
}

pub fn find_pairs(x: &[f64], y: &[f64], radius: &[f64]) -> (Vec<i64>, Vec<i64>) {
    let mut first = Vec::new();
    let mut second = Vec::new();
    let count = x.len();
    if count < 2 {
        return (first, second);
    }

    // the grid
    let mut low_x = x[0];
    let mut high_x = x[0];
    let mut low_y = y[0];
    let mut high_y = y[0];
    for row in 1..count {
        low_x = low_x.min(x[row]);
        high_x = high_x.max(x[row]);
        low_y = low_y.min(y[row]);
        high_y = high_y.max(y[row]);
    }
    let area = ((high_x - low_x) * (high_y - low_y)).max(1e-12);
    let square = (2.0 * quantile(radius, 0.9)).max((area / count as f64).sqrt()).max(1e-9);
    let columns = ((high_x - low_x) / square) as usize + 1;
    let rows = ((high_y - low_y) / square) as usize + 1;

    // file every circle under its square (a counting sort)
    let mut square_of = Vec::with_capacity(count);
    let mut in_square = vec![0usize; columns * rows + 1];
    for row in 0..count {
        let column = (((x[row] - low_x) / square) as usize).min(columns - 1);
        let row_of_squares = (((y[row] - low_y) / square) as usize).min(rows - 1);
        let place = column * rows + row_of_squares;
        square_of.push(place);
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
    let mut grid_x = Vec::with_capacity(count);
    let mut grid_y = Vec::with_capacity(count);
    let mut grid_radius = Vec::with_capacity(count);
    for &row in &filed {
        grid_x.push(x[row]);
        grid_y.push(y[row]);
        grid_radius.push(radius[row]);
    }

    // each circle, in grid order, looks for circles no bigger than itself
    for place in 0..count {
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
    let mut winners = Vec::new();
    let mut losers = Vec::new();
    for pair in 0..first.len() {
        let a = first[pair] as usize;
        let b = second[pair] as usize;
        let (winner, loser) = if stronger(a, b, mass, birth, ids) { (a, b) } else { (b, a) };
        if loser < owned {
            winners.push(winner as i64);
            losers.push(loser as i64);
        }
    }
    (winners, losers)
}
