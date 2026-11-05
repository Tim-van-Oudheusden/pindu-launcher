//! Fullscreen mode's on-screen keyboard for the search field: a grid of keys the D-pad
//! moves through and A types, B deletes with, and Start confirms.

use crate::gamepad::{Direction, Intent};

/// What a key on the keyboard does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Space,
    Backspace,
    Done,
}

/// A key where it sits in the grid: its first column and how many columns it covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub key: Key,
    pub column: usize,
    pub width: usize,
}

/// How many columns the grid has.
pub const COLUMNS: usize = 10;

/// The rows of characters, digits over a QWERTY layout; the last two rows end in wide
/// Backspace and Done keys under Space.
const CHARACTERS: [&str; 4] = ["1234567890", "qwertyuiop", "asdfghjkl", "zxcvbnm"];
const ROWS: usize = CHARACTERS.len() + 1;
/// Where the wide keys start: Backspace after "zxcvbnm", Done after Space.
const WIDE_START: usize = 7;

/// How many keys row `row` has.
fn row_len(row: usize) -> usize {
    match CHARACTERS.get(row) {
        Some(characters) if row == CHARACTERS.len() - 1 => characters.len() + 1,
        Some(characters) => characters.len(),
        None => 2,
    }
}

/// The key at `index` in row `row`.
fn cell(row: usize, index: usize) -> Cell {
    let wide = |key, column| Cell {
        key,
        column,
        width: if column == 0 {
            WIDE_START
        } else {
            COLUMNS - WIDE_START
        },
    };
    match CHARACTERS.get(row) {
        Some(characters) => match characters.chars().nth(index) {
            Some(character) => Cell {
                key: Key::Char(character),
                column: index,
                width: 1,
            },
            None => wide(Key::Backspace, WIDE_START),
        },
        None if index == 0 => wide(Key::Space, 0),
        None => wide(Key::Done, WIDE_START),
    }
}

/// The keyboard's rows, top to bottom, each a run of keys from the first column.
pub fn rows() -> impl Iterator<Item = impl Iterator<Item = Cell>> {
    (0..ROWS).map(|row| (0..row_len(row)).map(move |index| cell(row, index)))
}

/// The keyboard's cursor. It keeps the column it is in while it moves up and down, so
/// passing over a wide key comes back to the same column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Keyboard {
    row: usize,
    column: usize,
}

impl Default for Keyboard {
    /// On the first letter.
    fn default() -> Self {
        Self { row: 1, column: 0 }
    }
}

impl Keyboard {
    /// The key the cursor is on.
    pub fn key(&self) -> Key {
        cell(self.row, self.index()).key
    }

    /// The cursor's row and the key's index in it.
    pub fn position(&self) -> (usize, usize) {
        (self.row, self.index())
    }

    /// The index of the key covering the cursor's column, or of the row's last key when
    /// the row is shorter.
    fn index(&self) -> usize {
        let len = row_len(self.row);
        (0..len)
            .find(|&index| {
                let cell = cell(self.row, index);
                (cell.column..cell.column + cell.width).contains(&self.column)
            })
            .unwrap_or(len - 1)
    }

    /// Moves the cursor a key `direction`, wrapping around at the grid's edges.
    pub fn step(&mut self, direction: Direction) {
        let len = row_len(self.row);
        let index = self.index();
        match direction {
            Direction::Left => self.column = cell(self.row, (index + len - 1) % len).column,
            Direction::Right => self.column = cell(self.row, (index + 1) % len).column,
            Direction::Up => self.row = (self.row + ROWS - 1) % ROWS,
            Direction::Down => self.row = (self.row + 1) % ROWS,
        }
    }

    /// Acts on a controller intent while the keyboard is open over a search field holding
    /// `text`, and says what the field and the keyboard do next.
    pub fn handle(&mut self, intent: Intent, text: &str) -> Outcome {
        let typed = |character| Outcome::Edit(format!("{text}{character}"));
        match intent {
            Intent::Activate => match self.key() {
                Key::Char(character) => typed(character),
                Key::Space => typed(' '),
                Key::Backspace => delete(text).unwrap_or(Outcome::Ignored),
                Key::Done => Outcome::Confirm,
            },
            Intent::Back => delete(text).unwrap_or(Outcome::Close),
            Intent::Menu => Outcome::Confirm,
            Intent::Navigate(direction) => {
                self.step(direction);
                Outcome::Moved
            }
            _ => Outcome::Ignored,
        }
    }
}

/// The field's text without its last character, unless it is empty.
fn delete(text: &str) -> Option<Outcome> {
    let mut characters = text.chars();
    characters
        .next_back()
        .map(|_| Outcome::Edit(characters.as_str().to_owned()))
}

/// What the search field and the keyboard do after an intent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The cursor moved to another key.
    Moved,
    /// The field's text becomes this.
    Edit(String),
    /// The keyboard closes, leaving the field empty.
    Close,
    /// The keyboard closes and focus goes to the results.
    Confirm,
    /// The keyboard does nothing with the intent.
    Ignored,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gamepad::Direction::{Down, Left, Right, Up};
    use crate::gamepad::Intent::{Activate, Back, Menu, Navigate, Search};

    /// The keyboard after `steps` from the first letter.
    fn on(steps: &[Direction]) -> Keyboard {
        let mut keyboard = Keyboard::default();
        for &direction in steps {
            keyboard.step(direction);
        }
        keyboard
    }

    /// Where the cursor lands after `steps` from the first letter.
    fn after(steps: &[Direction]) -> Key {
        on(steps).key()
    }

    #[test]
    fn left_and_right_move_along_a_row_and_wrap_around_its_ends() {
        assert_eq!(after(&[]), Key::Char('q'));
        assert_eq!(after(&[Right]), Key::Char('w'));
        assert_eq!(after(&[Left]), Key::Char('p'));
        assert_eq!(after(&[Left, Right]), Key::Char('q'));
        assert_eq!(after(&[Down, Down, Left]), Key::Backspace);
        assert_eq!(after(&[Down, Down, Left, Left]), Key::Char('m'));
        assert_eq!(after(&[Down, Down, Down, Right]), Key::Done);
        assert_eq!(after(&[Down, Down, Down, Right, Right]), Key::Space);
    }

    #[test]
    fn up_and_down_keep_the_column_and_wrap_between_the_top_and_bottom_rows() {
        assert_eq!(after(&[Up]), Key::Char('1'));
        assert_eq!(after(&[Up, Up]), Key::Space);
        assert_eq!(after(&[Down, Down, Down, Down]), Key::Char('1'));
        // From "m" down to the wide Space and on round to the "7" above "m".
        let m = [Down, Down, Right, Right, Right, Right, Right, Right];
        assert_eq!(after(&m), Key::Char('m'));
        assert_eq!(after(&[&m[..], &[Down]].concat()), Key::Space);
        assert_eq!(after(&[&m[..], &[Down, Down]].concat()), Key::Char('7'));
        // The short "asdfghjkl" row: down from "p" lands on its last key, "l", and on
        // down to the Backspace under the "p" column.
        assert_eq!(after(&[Left, Down]), Key::Char('l'));
        assert_eq!(after(&[Left, Down, Down]), Key::Backspace);
        assert_eq!(after(&[Left, Down, Up]), Key::Char('p'));
    }

    #[test]
    fn a_types_the_key_deletes_on_backspace_and_confirms_on_done() {
        let edit = |text: &str| Outcome::Edit(text.to_owned());
        assert_eq!(on(&[]).handle(Activate, "ha"), edit("haq"));
        assert_eq!(
            on(&[Up]).handle(Activate, "half life "),
            edit("half life 1")
        );
        assert_eq!(on(&[Up, Up]).handle(Activate, "half"), edit("half "));
        assert_eq!(
            on(&[Down, Down, Left]).handle(Activate, "café"),
            edit("caf")
        );
        assert_eq!(
            on(&[Down, Down, Left]).handle(Activate, ""),
            Outcome::Ignored
        );
        assert_eq!(
            on(&[Up, Up, Right]).handle(Activate, "portal"),
            Outcome::Confirm
        );
    }

    #[test]
    fn b_deletes_a_character_and_closes_the_keyboard_once_the_field_is_empty() {
        let mut keyboard = on(&[Right]);
        assert_eq!(keyboard.handle(Back, "né"), Outcome::Edit("n".to_owned()));
        assert_eq!(keyboard.handle(Back, "n"), Outcome::Edit(String::new()));
        assert_eq!(keyboard.handle(Back, ""), Outcome::Close);
        // B deletes wherever the cursor is.
        assert_eq!(keyboard.key(), Key::Char('w'));
    }

    #[test]
    fn start_confirms_the_d_pad_moves_and_other_buttons_do_nothing() {
        let mut keyboard = Keyboard::default();
        assert_eq!(keyboard.handle(Menu, "portal"), Outcome::Confirm);
        assert_eq!(keyboard.handle(Menu, ""), Outcome::Confirm);
        assert_eq!(keyboard.handle(Navigate(Right), "portal"), Outcome::Moved);
        assert_eq!(keyboard.key(), Key::Char('w'));
        assert_eq!(keyboard.handle(Search, "portal"), Outcome::Ignored);
        assert_eq!(keyboard.key(), Key::Char('w'));
    }

    #[test]
    fn the_keyboard_has_letters_digits_space_backspace_and_done_once_each() {
        let keys: Vec<Key> = rows().flatten().map(|cell| cell.key).collect();
        let mut expected: Vec<Key> = ('a'..='z').chain('0'..='9').map(Key::Char).collect();
        expected.extend([Key::Space, Key::Backspace, Key::Done]);
        assert_eq!(keys.len(), expected.len());
        for key in expected {
            assert!(keys.contains(&key), "{key:?} is missing");
        }
    }
}
