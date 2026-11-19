//! Reading a long text with a controller: while the text has focus, Up and Down scroll
//! through it a step at a time, and move focus on only once its end in that direction
//! shows. A text taller than the screen could not be read to its end otherwise, as moving
//! focus onto it shows only its top.

use crate::gamepad::Direction;

/// The part of a scrolled page that shows, in the page's coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    /// The page's coordinate at the top of the view: the scroll position.
    pub top: f64,
    /// The view's height.
    pub height: f64,
}

/// Where the focused text lies on the page, in the page's coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Span {
    /// Its top.
    pub top: f64,
    /// Its bottom.
    pub bottom: f64,
}

/// What a direction does while a long text has focus.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Step {
    /// Scroll the page to this position; focus stays on the text.
    Scroll(f64),
    /// Nothing: the text's end shows and nothing lies below it to move focus to.
    Stay,
    /// Move focus on, as the direction does elsewhere.
    MoveOn,
}

/// What `direction` does while the text at `text` has focus and `view` shows: Up and Down
/// scroll half the view's height towards the text's end in that direction while that end
/// is out of view, never past it; once it shows, focus moves on, or below the text stays
/// when `more_below` says nothing lies below it to move to. Left and Right move on.
pub fn step(view: View, text: Span, direction: Direction, more_below: bool) -> Step {
    let half = view.height / 2.0;
    match direction {
        Direction::Down if text.bottom > view.top + view.height + SLACK => {
            Step::Scroll((view.top + half).min(text.bottom - view.height))
        }
        Direction::Down if !more_below => Step::Stay,
        Direction::Up if text.top < view.top - SLACK => {
            Step::Scroll((view.top - half).max(text.top))
        }
        _ => Step::MoveOn,
    }
}

/// How far out of view, in pixels, an end may lie and still count as showing: layout
/// rounds to whole pixels.
const SLACK: f64 = 1.0;

#[cfg(test)]
mod tests {
    use super::*;

    const VIEW: View = View {
        top: 100.0,
        height: 400.0,
    };

    #[test]
    fn down_scrolls_half_a_screen_while_the_texts_end_is_below_the_view() {
        let text = Span {
            top: 200.0,
            bottom: 1200.0,
        };

        assert_eq!(step(VIEW, text, Direction::Down, true), Step::Scroll(300.0));
    }

    #[test]
    fn down_stops_where_the_texts_end_reaches_the_bottom_of_the_view() {
        let text = Span {
            top: 200.0,
            bottom: 600.0,
        };

        assert_eq!(step(VIEW, text, Direction::Down, true), Step::Scroll(200.0));
    }

    #[test]
    fn down_moves_on_once_the_texts_end_shows() {
        let text = Span {
            top: 200.0,
            bottom: 500.0,
        };

        assert_eq!(step(VIEW, text, Direction::Down, true), Step::MoveOn);
    }

    #[test]
    fn down_stays_at_the_texts_end_with_nothing_below_it() {
        let text = Span {
            top: 200.0,
            bottom: 500.0,
        };

        assert_eq!(step(VIEW, text, Direction::Down, false), Step::Stay);
    }

    #[test]
    fn up_scrolls_back_towards_the_texts_start_but_not_past_it() {
        let text = Span {
            top: 0.0,
            bottom: 1200.0,
        };
        let lower = View {
            top: 600.0,
            height: 400.0,
        };

        assert_eq!(step(lower, text, Direction::Up, true), Step::Scroll(400.0));
        assert_eq!(step(VIEW, text, Direction::Up, true), Step::Scroll(0.0));
    }

    #[test]
    fn up_moves_on_once_the_texts_start_shows() {
        let text = Span {
            top: 100.0,
            bottom: 1200.0,
        };

        assert_eq!(step(VIEW, text, Direction::Up, false), Step::MoveOn);
    }

    #[test]
    fn left_and_right_move_on() {
        let text = Span {
            top: 0.0,
            bottom: 1200.0,
        };

        assert_eq!(step(VIEW, text, Direction::Left, true), Step::MoveOn);
        assert_eq!(step(VIEW, text, Direction::Right, false), Step::MoveOn);
    }
}
