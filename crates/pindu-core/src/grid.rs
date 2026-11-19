//! Cover grid sizing and controller steps through it.

use crate::gamepad::Direction;

/// The most columns a cover grid should allow in `available` pixels, for columns
/// `column_width` pixels wide: one more than fit, so the grid's own count never gets cut.
/// GTK's grid keeps widgets for its maximum column count times many rows, so a cap far
/// above what fits makes it build, bind and drop several times the tiles it needs.
pub fn max_columns(available: f64, column_width: i32) -> u32 {
    let fit = (available / f64::from(column_width.max(1)))
        .floor()
        .max(0.0) as u32;
    fit + 1
}

/// The columns `column_width` pixels wide that fit in `available` pixels, at least one,
/// and the whole-pixel margin on either side that centres them. Columns exactly as wide as
/// a cover plus its gap keep the gap between columns the gap between rows, whatever the
/// width; what is left over goes to the sides.
pub fn centred_columns(available: f64, column_width: i32) -> (u32, i32) {
    let column_width = f64::from(column_width.max(1));
    let columns = (available / column_width).floor().max(1.0);
    let margin = ((available - columns * column_width) / 2.0)
        .floor()
        .max(0.0);
    (columns as u32, margin as i32)
}

/// The cover a controller direction moves to from `position`, in a grid of `n_items`
/// covers laid out `columns` to a row; `None` at the grid's edge, where focus leaves it.
/// Down into a short last row lands on its last cover.
pub fn step(position: u32, direction: Direction, n_items: u32, columns: u32) -> Option<u32> {
    let columns = columns.max(1);
    let column = position % columns;
    match direction {
        Direction::Left => (column > 0).then(|| position - 1),
        Direction::Right => (column + 1 < columns && position + 1 < n_items).then(|| position + 1),
        Direction::Up => position.checked_sub(columns),
        Direction::Down => {
            let next_row = (position / columns + 1) * columns;
            (next_row < n_items).then(|| (position + columns).min(n_items - 1))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_column_cap_is_one_more_than_the_columns_that_fit() {
        assert_eq!(max_columns(1000.0, 212), 5);
        assert_eq!(max_columns(636.0, 212), 4);
        assert_eq!(max_columns(100.0, 212), 1);
        assert_eq!(max_columns(0.0, 212), 1);
    }

    #[test]
    fn centred_columns_fill_the_width_and_split_what_is_left_between_the_sides() {
        // Four columns of 232 take 928 of 1000: 36 on either side.
        assert_eq!(centred_columns(1000.0, 232), (4, 36));
        // An odd leftover rounds the sides down, so the columns always fit.
        assert_eq!(centred_columns(929.0, 232), (4, 0));
        assert_eq!(centred_columns(931.5, 232), (4, 1));
        // Narrower than a column: one column, no margin.
        assert_eq!(centred_columns(100.0, 232), (1, 0));
        assert_eq!(centred_columns(0.0, 232), (1, 0));
    }

    #[test]
    fn a_direction_steps_to_the_neighbouring_cover_or_leaves_the_grid_at_its_edges() {
        use crate::gamepad::Direction::{Down, Left, Right, Up};
        // Ten covers in rows of four: 0-3, 4-7, 8-9.
        let step = |position, direction| step(position, direction, 10, 4);

        assert_eq!(step(5, Right), Some(6));
        assert_eq!(step(5, Left), Some(4));
        assert_eq!(step(5, Up), Some(1));
        assert_eq!(step(5, Down), Some(9));
        // The last row is short: down lands on its last cover.
        assert_eq!(step(7, Down), Some(9));
        assert_eq!(step(4, Left), None);
        assert_eq!(step(3, Right), None);
        assert_eq!(step(9, Right), None);
        assert_eq!(step(2, Up), None);
        assert_eq!(step(8, Down), None);
    }
}
