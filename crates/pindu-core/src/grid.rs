//! Cover grid sizing.

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
}
