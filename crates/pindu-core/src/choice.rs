//! A choice row: one setting whose value Left and Right step through in place, between
//! its first and last option, and which A steps on, wrapping from the last option to the
//! first. Options are counted by their place in the row's list, from 0.

/// The option before `index` among `count` options, or none when `index` is the first
/// (or not one of them).
pub fn previous(index: usize, count: usize) -> Option<usize> {
    index.checked_sub(1).filter(|_| index < count)
}

/// The option after `index` among `count` options, or none when `index` is the last (or
/// not one of them).
pub fn next(index: usize, count: usize) -> Option<usize> {
    let after = index.checked_add(1)?;
    (after < count).then_some(after)
}

/// The option A chooses after `index` among `count` options: the next one, or the first
/// after the last (or when `index` is not one of them); none when there are no options.
pub fn cycle(index: usize, count: usize) -> Option<usize> {
    match count {
        0 => None,
        _ => Some(next(index, count).unwrap_or(0)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn left_chooses_the_option_before_and_stops_at_the_first() {
        assert_eq!(previous(2, 3), Some(1));
        assert_eq!(previous(1, 3), Some(0));
        assert_eq!(previous(0, 3), None, "the first option has none before it");
        assert_eq!(
            previous(3, 3),
            None,
            "an index past the options is none of them"
        );
        assert_eq!(previous(0, 0), None);
    }

    #[test]
    fn right_chooses_the_option_after_and_stops_at_the_last() {
        assert_eq!(next(0, 3), Some(1));
        assert_eq!(next(1, 3), Some(2));
        assert_eq!(next(2, 3), None, "the last option has none after it");
        assert_eq!(next(0, 1), None, "a lone option is both first and last");
        assert_eq!(
            next(5, 3),
            None,
            "an index past the options is none of them"
        );
        assert_eq!(next(0, 0), None);
    }

    #[test]
    fn a_chooses_the_next_option_and_wraps_from_the_last_to_the_first() {
        assert_eq!(cycle(0, 4), Some(1));
        assert_eq!(cycle(2, 4), Some(3));
        assert_eq!(cycle(3, 4), Some(0), "the last option wraps to the first");
        assert_eq!(cycle(0, 1), Some(0), "a lone option stays chosen");
        assert_eq!(
            cycle(7, 4),
            Some(0),
            "an index past the options goes to the first"
        );
        assert_eq!(cycle(0, 0), None, "no options, nothing to choose");
    }
}
