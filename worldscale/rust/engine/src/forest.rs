// The trees and seeds of one rank's part of the world, as a table: one
// column (a Vec) per value, one row per tree or seed, the same columns as
// worldscale/forest.py's Forest.
//
// `Forest` owns its columns. `Trees` borrows every column (or a run of rows
// of every column) for the growth sums to work on, so the same sums work on
// a Forest here and on the numpy arrays of a Python Forest, and a table can
// be cut into pieces for the cores of a computer to work on at once.
//
// Letters (the bytes ranks send each other) are written and read here too.

/// How many cycles of growth a plant remembers, at most (forest.MEMORY_SLOTS)
pub const MEMORY_SLOTS: usize = 4;

/// Tables smaller than this are compacted one column after another, bigger
/// ones with a column on each core
const PARALLEL_KEEP_ROWS: usize = 20_000;

// Every column is listed once, here, and the macro writes the Forest and
// Trees structs and the code that does the same thing to every column.
macro_rules! columns {
    ( $( $name:ident : $kind:ty ),* $(,)? ) => {
        /// A table of trees and seeds
        #[derive(Clone, Default)]
        pub struct Forest {
            $( pub $name: Vec<$kind>, )*
            /// MEMORY_SLOTS values per row, the newest last
            pub fixed_record: Vec<f64>,
            pub height_record: Vec<f64>,
        }

        /// Every column of a table (or a run of its rows), borrowed for writing
        pub struct Trees<'a> {
            $( pub $name: &'a mut [$kind], )*
            pub fixed_record: &'a mut [f64],
            pub height_record: &'a mut [f64],
        }

        impl Forest {
            /// A table of `count` rows, all zero
            pub fn zeros(count: usize) -> Forest {
                Forest {
                    $( $name: vec![Default::default(); count], )*
                    fixed_record: vec![0.0; count * MEMORY_SLOTS],
                    height_record: vec![0.0; count * MEMORY_SLOTS],
                }
            }

            pub fn trees(&mut self) -> Trees<'_> {
                Trees {
                    $( $name: &mut self.$name, )*
                    fixed_record: &mut self.fixed_record,
                    height_record: &mut self.height_record,
                }
            }

            /// These rows, in this order, as a new table
            pub fn select(&self, rows: &[usize]) -> Forest {
                Forest {
                    $( $name: rows.iter().map(|&row| self.$name[row]).collect(), )*
                    fixed_record: select_records(&self.fixed_record, rows),
                    height_record: select_records(&self.height_record, rows),
                }
            }

            /// Put the rows in this order (every row, once each). A column at a
            /// time is copied, so the table never takes much more memory.
            pub fn reorder(&mut self, rows: &[usize]) {
                let Forest { $( $name, )* fixed_record, height_record } = self;
                rayon::scope(|scope| {
                    $( scope.spawn(move |_| {
                        // (keeping the room the column had, for the rows to come)
                        let mut moved = Vec::with_capacity($name.capacity());
                        moved.extend(rows.iter().map(|&row| $name[row]));
                        *$name = moved;
                    }); )*
                    scope.spawn(move |_| reorder_records(fixed_record, rows));
                    scope.spawn(move |_| reorder_records(height_record, rows));
                });
            }

            /// Keep the rows where mask[row] is `value`, and take the rest
            /// out: the rows kept move to the front, in order
            pub fn keep_where(&mut self, mask: &[bool], value: bool) {
                let kept = self.trees().keep_where(mask, value);
                $( self.$name.truncate(kept); )*
                self.fixed_record.truncate(kept * MEMORY_SLOTS);
                self.height_record.truncate(kept * MEMORY_SLOTS);
            }

            /// Make every column `count` rows long, filling new rows with zeros
            /// (columns already that long are left as they are)
            pub fn fill_to(&mut self, count: usize) {
                $( self.$name.resize(count, Default::default()); )*
                self.fixed_record.resize(count * MEMORY_SLOTS, 0.0);
                self.height_record.resize(count * MEMORY_SLOTS, 0.0);
            }
        }

        impl<'a> Trees<'a> {
            /// The same columns, borrowed again (for a shorter time)
            pub fn reborrow(&mut self) -> Trees<'_> {
                Trees {
                    $( $name: &mut *self.$name, )*
                    fixed_record: &mut *self.fixed_record,
                    height_record: &mut *self.height_record,
                }
            }

            /// The rows before `row`, and the rest
            pub fn split_at(self, row: usize) -> (Trees<'a>, Trees<'a>) {
                $( let $name = self.$name.split_at_mut(row); )*
                let fixed_record = self.fixed_record.split_at_mut(row * MEMORY_SLOTS);
                let height_record = self.height_record.split_at_mut(row * MEMORY_SLOTS);
                (Trees { $( $name: $name.0, )* fixed_record: fixed_record.0, height_record: height_record.0 },
                 Trees { $( $name: $name.1, )* fixed_record: fixed_record.1, height_record: height_record.1 })
            }

            /// Move the rows where mask[row] is `value` to the front of every
            /// column, in order. Gives back how many there are.
            pub fn keep_where(&mut self, mask: &[bool], value: bool) -> usize {
                let runs = runs_where(mask, value);
                let runs = &runs;
                let Trees { $( $name, )* fixed_record, height_record } = self;
                if mask.len() < PARALLEL_KEEP_ROWS {
                    $( move_runs($name, runs, 1); )*
                    move_runs(fixed_record, runs, MEMORY_SLOTS);
                    move_runs(height_record, runs, MEMORY_SLOTS);
                } else {
                    rayon::scope(|scope| {
                        $( scope.spawn(move |_| move_runs($name, runs, 1)); )*
                        scope.spawn(move |_| move_runs(fixed_record, runs, MEMORY_SLOTS));
                        scope.spawn(move |_| move_runs(height_record, runs, MEMORY_SLOTS));
                    });
                }
                runs.iter().map(|&(start, end)| end - start).sum()
            }
        }
    };
}

columns! {
    id: u64,
    species: i32,
    is_seed: bool,
    x: f64,
    y: f64,
    birth_cycle: i32,
    age: i32,
    count_to_germ: i32,
    mass_seed: f64,
    radius_seed: f64,
    mass_stem: f64,
    mass_leaf: f64,
    mass_fixed: f64,
    mass_total: f64,
    radius_stem: f64,
    radius_leaf: f64,
    r: f64,
    height_stem: f64,
    is_mature: bool,
    area_covered: f64,
    fixed_count: i32,
    height_count: i32,
    prev_height: f64,
    avg_height_growth: f64,
    max_avg_height_growth: f64,
    attached_count: i32,
    attached_mass: f64,
}

impl Forest {
    pub fn len(&self) -> usize {
        self.id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.id.is_empty()
    }
}

impl Trees<'_> {
    pub fn len(&self) -> usize {
        self.id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.id.is_empty()
    }
}

/// The runs of rows where mask[row] is `value`: (first row, row after the
/// last) of each, in order
fn runs_where(mask: &[bool], value: bool) -> Vec<(usize, usize)> {
    let mut runs = Vec::new();
    let mut row = 0;
    while row < mask.len() {
        if mask[row] == value {
            let start = row;
            while row < mask.len() && mask[row] == value {
                row += 1;
            }
            runs.push((start, row));
        } else {
            row += 1;
        }
    }
    runs
}

/// Move these runs of rows (each `width` values) to the front of a column,
/// one after another, in order
fn move_runs<T: Copy>(column: &mut [T], runs: &[(usize, usize)], width: usize) {
    let mut kept = 0;
    for &(start, end) in runs {
        if start != kept {
            column.copy_within(start * width..end * width, kept * width);
        }
        kept += end - start;
    }
}

fn reorder_records(record: &mut Vec<f64>, rows: &[usize]) {
    let mut moved = Vec::with_capacity(record.capacity());
    for &row in rows {
        moved.extend_from_slice(&record[row * MEMORY_SLOTS..(row + 1) * MEMORY_SLOTS]);
    }
    *record = moved;
}

fn select_records(record: &[f64], rows: &[usize]) -> Vec<f64> {
    let mut chosen = Vec::with_capacity(rows.len() * MEMORY_SLOTS);
    for &row in rows {
        chosen.extend_from_slice(&record[row * MEMORY_SLOTS..(row + 1) * MEMORY_SLOTS]);
    }
    chosen
}

// ---------------------------------------------------------------------
// Letters: what ranks send each other, as bytes
// ---------------------------------------------------------------------

/// A value that can go in a letter
pub trait Wire: Copy {
    const SIZE: usize;
    fn write(self, letter: &mut Vec<u8>);
    fn read(bytes: &[u8]) -> Self;
}

impl Wire for u64 {
    const SIZE: usize = 8;
    fn write(self, letter: &mut Vec<u8>) {
        letter.extend_from_slice(&self.to_le_bytes());
    }
    fn read(bytes: &[u8]) -> u64 {
        u64::from_le_bytes(bytes[..8].try_into().unwrap())
    }
}

impl Wire for f64 {
    const SIZE: usize = 8;
    fn write(self, letter: &mut Vec<u8>) {
        letter.extend_from_slice(&self.to_le_bytes());
    }
    fn read(bytes: &[u8]) -> f64 {
        f64::from_le_bytes(bytes[..8].try_into().unwrap())
    }
}

impl Wire for i32 {
    const SIZE: usize = 4;
    fn write(self, letter: &mut Vec<u8>) {
        letter.extend_from_slice(&self.to_le_bytes());
    }
    fn read(bytes: &[u8]) -> i32 {
        i32::from_le_bytes(bytes[..4].try_into().unwrap())
    }
}

impl Wire for u8 {
    const SIZE: usize = 1;
    fn write(self, letter: &mut Vec<u8>) {
        letter.push(self);
    }
    fn read(bytes: &[u8]) -> u8 {
        bytes[0]
    }
}

/// Write a column's values for these rows
pub fn write_column<T: Wire>(letter: &mut Vec<u8>, column: &[T], rows: &[usize]) {
    letter.reserve(rows.len() * T::SIZE);
    for &row in rows {
        column[row].write(letter);
    }
}

/// Reads a letter's values back, in the order they were written
pub struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Reader<'a> {
        Reader { bytes, at: 0 }
    }

    pub fn value<T: Wire>(&mut self) -> T {
        let value = T::read(&self.bytes[self.at..]);
        self.at += T::SIZE;
        value
    }

    /// `count` values, added to the end of a column
    pub fn column<T: Wire>(&mut self, count: usize, into: &mut Vec<T>) {
        let bytes = &self.bytes[self.at..self.at + count * T::SIZE];
        into.extend(bytes.chunks_exact(T::SIZE).map(T::read));
        self.at += count * T::SIZE;
    }

    pub fn finished(&self) -> bool {
        self.at == self.bytes.len()
    }
}
