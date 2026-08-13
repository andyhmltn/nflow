use crate::hint::labels::ALPHABET;
use crate::types::Rect;

pub const GRID_COLS: usize = 4;
pub const GRID_ROWS: usize = 4;
pub const CELL_COUNT: usize = GRID_COLS * GRID_ROWS;

const MIN_CELL_DIM: f64 = 4.0;

pub fn subdivide(bounds: Rect) -> Vec<Rect> {
    let cell_w = bounds.width / GRID_COLS as f64;
    let cell_h = bounds.height / GRID_ROWS as f64;
    let mut cells = Vec::with_capacity(CELL_COUNT);
    for row in 0..GRID_ROWS {
        for col in 0..GRID_COLS {
            cells.push(Rect {
                x: bounds.x + col as f64 * cell_w,
                y: bounds.y + row as f64 * cell_h,
                width: cell_w,
                height: cell_h,
            });
        }
    }
    cells
}

pub fn labels() -> Vec<String> {
    ALPHABET
        .iter()
        .take(CELL_COUNT)
        .map(|c| c.to_string())
        .collect()
}

pub fn is_terminal(bounds: Rect) -> bool {
    bounds.width / GRID_COLS as f64 <= MIN_CELL_DIM
        || bounds.height / GRID_ROWS as f64 <= MIN_CELL_DIM
}

pub fn centre(bounds: Rect) -> (f64, f64) {
    (
        bounds.x + bounds.width / 2.0,
        bounds.y + bounds.height / 2.0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect {
            x,
            y,
            width: w,
            height: h,
        }
    }

    #[test]
    fn subdivide_produces_sixteen_cells() {
        let cells = subdivide(r(0.0, 0.0, 400.0, 400.0));
        assert_eq!(cells.len(), CELL_COUNT);
    }

    #[test]
    fn subdivide_cells_tile_the_bounds() {
        let cells = subdivide(r(0.0, 0.0, 400.0, 400.0));
        assert_eq!(cells[0], r(0.0, 0.0, 100.0, 100.0));
        assert_eq!(cells[3], r(300.0, 0.0, 100.0, 100.0));
        assert_eq!(cells[12], r(0.0, 300.0, 100.0, 100.0));
        assert_eq!(cells[15], r(300.0, 300.0, 100.0, 100.0));
    }

    #[test]
    fn subdivide_handles_non_square_bounds() {
        let cells = subdivide(r(10.0, 20.0, 800.0, 400.0));
        assert_eq!(cells[0], r(10.0, 20.0, 200.0, 100.0));
        assert_eq!(cells[15], r(610.0, 320.0, 200.0, 100.0));
    }

    #[test]
    fn labels_are_home_row_first() {
        let l = labels();
        assert_eq!(l.len(), CELL_COUNT);
        assert_eq!(l[0], "a");
        assert_eq!(l[1], "s");
        assert_eq!(l[2], "d");
        assert_eq!(l[3], "f");
    }

    #[test]
    fn labels_are_unique() {
        let l = labels();
        for i in 0..l.len() {
            for j in (i + 1)..l.len() {
                assert_ne!(l[i], l[j]);
            }
        }
    }

    #[test]
    fn terminal_when_cell_dimension_hits_floor() {
        assert!(is_terminal(r(0.0, 0.0, 16.0, 16.0)));
        assert!(!is_terminal(r(0.0, 0.0, 17.0, 17.0)));
    }

    #[test]
    fn centre_is_bounds_midpoint() {
        assert_eq!(centre(r(0.0, 0.0, 400.0, 200.0)), (200.0, 100.0));
        assert_eq!(centre(r(100.0, 100.0, 50.0, 50.0)), (125.0, 125.0));
    }
}
