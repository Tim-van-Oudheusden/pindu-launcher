//! Fullscreen mode's on-screen keyboard: a grid of keys the D-pad moves through and A
//! types, B deletes with, and Start confirms. The search keyboard has one layer of digits,
//! lowercase letters and space. The passphrase keyboard, for Wi-Fi passphrases and
//! Bluetooth PINs, has a lowercase, a capitals and a symbols layer, which keys on its
//! bottom row switch between, so it types every printable ASCII character; it hides the
//! text it types until its Show key shows it.

use crate::gamepad::{Direction, Intent};

/// What a key on the keyboard does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    /// Types a character.
    Char(char),
    /// Types a space.
    Space,
    /// Deletes the last character.
    Backspace,
    /// Confirms the text.
    Done,
    /// Switches the keyboard to a layer.
    Layer(Layer),
    /// Shows the hidden text, or hides it again.
    Reveal,
}

/// A set of keys the keyboard shows at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Layer {
    /// The search keyboard's only layer: digits, lowercase letters and space.
    Search,
    /// The passphrase keyboard's digits and lowercase letters.
    Lower,
    /// The passphrase keyboard's digits and capitals.
    Upper,
    /// The passphrase keyboard's symbols: the rest of printable ASCII.
    Symbols,
}

/// A key where it sits in the grid: its first column and how many columns it covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    /// The key.
    pub key: Key,
    /// Its first column, from 0.
    pub column: usize,
    /// How many columns it covers.
    pub width: usize,
}

/// How many columns the grid has.
pub const COLUMNS: usize = 10;

/// One row of a layer: a key per character from the first column, then wider keys with
/// how many columns each covers.
struct Row {
    characters: &'static str,
    wide: &'static [(Key, usize)],
}

impl Row {
    /// A row of characters only.
    const fn characters(characters: &'static str) -> Self {
        Self {
            characters,
            wide: &[],
        }
    }

    /// A row of characters that ends in a Backspace under the last three columns.
    const fn backspace(characters: &'static str) -> Self {
        Self {
            characters,
            wide: &[(Key::Backspace, 3)],
        }
    }

    /// How many keys the row has.
    fn len(&self) -> usize {
        self.characters.chars().count() + self.wide.len()
    }

    /// The key at `index`, which must be less than the row's length.
    fn cell(&self, index: usize) -> Cell {
        if let Some(character) = self.characters.chars().nth(index) {
            return Cell {
                key: Key::Char(character),
                column: index,
                width: 1,
            };
        }
        let characters = self.characters.chars().count();
        let (before, rest) = self.wide.split_at(index - characters);
        let (key, width) = rest[0];
        Cell {
            key,
            column: characters + before.iter().map(|&(_, width)| width).sum::<usize>(),
            width,
        }
    }
}

/// The search keyboard: digits over a QWERTY layout; the last two rows end in wide
/// Backspace and Done keys under Space.
const SEARCH: [Row; 5] = [
    Row::characters("1234567890"),
    Row::characters("qwertyuiop"),
    Row::characters("asdfghjkl"),
    Row::backspace("zxcvbnm"),
    Row {
        characters: "",
        wide: &[(Key::Space, 7), (Key::Done, 3)],
    },
];

/// The passphrase keyboard's lowercase layer: the search keyboard's letters and digits,
/// over a bottom row of two layer keys, Space, Show and Done; the first layer key switches
/// to capitals and the second to symbols.
const LOWER: [Row; 5] = [
    Row::characters("1234567890"),
    Row::characters("qwertyuiop"),
    Row::characters("asdfghjkl"),
    Row::backspace("zxcvbnm"),
    Row {
        characters: "",
        wide: &[
            (Key::Layer(Layer::Upper), 2),
            (Key::Layer(Layer::Symbols), 2),
            (Key::Space, 2),
            (Key::Reveal, 2),
            (Key::Done, 2),
        ],
    },
];

/// The capitals layer, whose first bottom key goes back to lowercase.
const UPPER: [Row; 5] = [
    Row::characters("1234567890"),
    Row::characters("QWERTYUIOP"),
    Row::characters("ASDFGHJKL"),
    Row::backspace("ZXCVBNM"),
    Row {
        characters: "",
        wide: &[
            (Key::Layer(Layer::Lower), 2),
            (Key::Layer(Layer::Symbols), 2),
            (Key::Space, 2),
            (Key::Reveal, 2),
            (Key::Done, 2),
        ],
    },
];

/// The symbols layer: the 32 printable ASCII symbols, with the Backspace where the
/// letter layers have it; its first bottom key goes to capitals, its second back to
/// lowercase.
const SYMBOLS: [Row; 5] = [
    Row::characters("!@#$%^&*()"),
    Row::characters("-_=+[]{}"),
    Row::characters("\\|;:'\"`"),
    Row::backspace("~,./<>?"),
    Row {
        characters: "",
        wide: &[
            (Key::Layer(Layer::Upper), 2),
            (Key::Layer(Layer::Lower), 2),
            (Key::Space, 2),
            (Key::Reveal, 2),
            (Key::Done, 2),
        ],
    },
];

impl Layer {
    /// The layer's rows of keys.
    fn table(self) -> &'static [Row] {
        match self {
            Layer::Search => &SEARCH,
            Layer::Lower => &LOWER,
            Layer::Upper => &UPPER,
            Layer::Symbols => &SYMBOLS,
        }
    }

    /// The layer's rows, top to bottom, each a run of keys from the first column.
    pub fn rows(self) -> impl Iterator<Item = impl Iterator<Item = Cell>> {
        self.table()
            .iter()
            .map(|row| (0..row.len()).map(move |index| row.cell(index)))
    }
}

/// The search keyboard's rows, top to bottom, each a run of keys from the first column.
pub fn rows() -> impl Iterator<Item = impl Iterator<Item = Cell>> {
    Layer::Search.rows()
}

/// The keyboard's cursor, the layer it shows and whether it hides the text. The cursor
/// keeps the column it is in while it moves up and down, so passing over a wide key comes
/// back to the same column, and it stays where it is when the layer changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Keyboard {
    layer: Layer,
    row: usize,
    column: usize,
    hidden: bool,
}

impl Default for Keyboard {
    /// The search keyboard, on the first letter.
    fn default() -> Self {
        Self {
            layer: Layer::Search,
            row: 1,
            column: 0,
            hidden: false,
        }
    }
}

impl Keyboard {
    /// The passphrase keyboard, on the first lowercase letter, hiding the text.
    pub fn passphrase() -> Self {
        Self {
            layer: Layer::Lower,
            hidden: true,
            ..Self::default()
        }
    }

    /// The layer the keyboard shows; [`Layer::rows`] lays it out.
    pub fn layer(&self) -> Layer {
        self.layer
    }

    /// Whether the text shows as dots.
    pub fn hidden(&self) -> bool {
        self.hidden
    }

    /// The row the cursor is in.
    fn row(&self) -> &'static Row {
        &self.layer.table()[self.row]
    }

    /// The key the cursor is on.
    pub fn key(&self) -> Key {
        self.row().cell(self.index()).key
    }

    /// The cursor's row and the key's index in it.
    pub fn position(&self) -> (usize, usize) {
        (self.row, self.index())
    }

    /// The index of the key covering the cursor's column, or of the row's last key when
    /// the row is shorter.
    fn index(&self) -> usize {
        let row = self.row();
        let len = row.len();
        (0..len)
            .find(|&index| {
                let cell = row.cell(index);
                (cell.column..cell.column + cell.width).contains(&self.column)
            })
            .unwrap_or(len - 1)
    }

    /// Moves the cursor a key `direction`, wrapping around at the grid's edges.
    pub fn step(&mut self, direction: Direction) {
        let row = self.row();
        let rows = self.layer.table().len();
        let len = row.len();
        let index = self.index();
        match direction {
            Direction::Left => self.column = row.cell((index + len - 1) % len).column,
            Direction::Right => self.column = row.cell((index + 1) % len).column,
            Direction::Up => self.row = (self.row + rows - 1) % rows,
            Direction::Down => self.row = (self.row + 1) % rows,
        }
    }

    /// Acts on a controller intent while the keyboard is open over a field holding
    /// `text`, and says what the field and the keyboard do next.
    pub fn handle(&mut self, intent: Intent, text: &str) -> Outcome {
        let typed = |character| Outcome::Edit(format!("{text}{character}"));
        match intent {
            Intent::Activate => match self.key() {
                Key::Char(character) => typed(character),
                Key::Space => typed(' '),
                Key::Backspace => delete(text).unwrap_or(Outcome::Ignored),
                Key::Done => Outcome::Confirm,
                Key::Layer(layer) => {
                    self.layer = layer;
                    Outcome::Layer
                }
                Key::Reveal => {
                    self.hidden = !self.hidden;
                    Outcome::Reveal
                }
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

/// What the field and the keyboard do after an intent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The cursor moved to another key.
    Moved,
    /// The field's text becomes this.
    Edit(String),
    /// The keyboard switched to another layer, as [`Keyboard::layer`] says; the cursor
    /// stays where it is.
    Layer,
    /// The text shows or hides, as [`Keyboard::hidden`] says.
    Reveal,
    /// The keyboard closes, leaving the field empty: the search ends, a dialog cancels.
    Close,
    /// The keyboard closes with the text: the search goes to its results, a dialog
    /// returns the text.
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

    /// The passphrase keyboard after `steps` from its first letter.
    fn passphrase_on(steps: &[Direction]) -> Keyboard {
        let mut keyboard = Keyboard::passphrase();
        for &direction in steps {
            keyboard.step(direction);
        }
        keyboard
    }

    /// The keys of `layer`, row by row.
    fn keys(layer: Layer) -> Vec<Vec<Key>> {
        layer
            .rows()
            .map(|row| row.map(|cell| cell.key).collect())
            .collect()
    }

    #[test]
    fn the_search_keyboard_keeps_its_one_layer_and_never_hides_the_text() {
        let chars = |row: &str| row.chars().map(Key::Char).collect::<Vec<_>>();
        let mut last = chars("zxcvbnm");
        last.push(Key::Backspace);
        assert_eq!(
            keys(Layer::Search),
            [
                chars("1234567890"),
                chars("qwertyuiop"),
                chars("asdfghjkl"),
                last,
                vec![Key::Space, Key::Done],
            ]
        );
        let keyboard = Keyboard::default();
        assert_eq!(keyboard.layer(), Layer::Search);
        assert!(!keyboard.hidden());
        let search: Vec<Vec<Cell>> = rows().map(|row| row.collect()).collect();
        let layer: Vec<Vec<Cell>> = Layer::Search.rows().map(|row| row.collect()).collect();
        assert_eq!(search, layer);
    }

    #[test]
    fn every_layer_fills_the_grid_without_gaps_or_overlaps() {
        for layer in [Layer::Search, Layer::Lower, Layer::Upper, Layer::Symbols] {
            for row in layer.rows() {
                let mut next = 0;
                for cell in row {
                    assert_eq!(cell.column, next, "{layer:?}: {cell:?} leaves a gap");
                    next += cell.width;
                }
                assert!(next <= COLUMNS, "{layer:?} is too wide");
            }
        }
        // The rows that end in a wide key fill the grid.
        for layer in [Layer::Lower, Layer::Upper, Layer::Symbols] {
            let ends: Vec<usize> = layer
                .rows()
                .map(|row| row.last().map_or(0, |cell| cell.column + cell.width))
                .collect();
            assert_eq!(ends[3..], [COLUMNS, COLUMNS], "{layer:?}");
        }
    }

    #[test]
    fn the_passphrase_layers_hold_every_printable_ascii_character_once() {
        let mut characters: Vec<char> = [Layer::Lower, Layer::Upper, Layer::Symbols]
            .into_iter()
            .flat_map(keys)
            .flatten()
            .filter_map(|key| match key {
                Key::Char(character) => Some(character),
                _ => None,
            })
            .collect();
        // The digits are on both letter layers.
        let digits = characters.iter().filter(|c| c.is_ascii_digit()).count();
        assert_eq!(digits, 20);
        characters.sort_unstable();
        characters.dedup();
        let printable: Vec<char> = ('!'..='~').collect();
        assert_eq!(characters, printable);
        for layer in [Layer::Lower, Layer::Upper, Layer::Symbols] {
            let layer_keys: Vec<Key> = keys(layer).into_iter().flatten().collect();
            for key in [Key::Space, Key::Backspace, Key::Done, Key::Reveal] {
                assert_eq!(
                    layer_keys.iter().filter(|&&k| k == key).count(),
                    1,
                    "{layer:?} has {key:?} once"
                );
            }
        }
    }

    #[test]
    fn the_layer_keys_switch_layers_and_the_cursor_stays_put() {
        let mut keyboard = passphrase_on(&[Down, Down, Down]);
        assert_eq!(keyboard.layer(), Layer::Lower);
        assert_eq!(keyboard.key(), Key::Layer(Layer::Upper));
        assert_eq!(keyboard.handle(Activate, "pass"), Outcome::Layer);
        assert_eq!(keyboard.layer(), Layer::Upper);
        assert_eq!(keyboard.position(), (4, 0));
        assert_eq!(keyboard.key(), Key::Layer(Layer::Lower));
        assert_eq!(keyboard.handle(Activate, "pass"), Outcome::Layer);
        assert_eq!(keyboard.layer(), Layer::Lower);
        // The second key goes to the symbols and back to lowercase.
        keyboard.step(Right);
        assert_eq!(keyboard.key(), Key::Layer(Layer::Symbols));
        assert_eq!(keyboard.handle(Activate, ""), Outcome::Layer);
        assert_eq!(keyboard.layer(), Layer::Symbols);
        assert_eq!(keyboard.key(), Key::Layer(Layer::Lower));
        keyboard.step(Left);
        assert_eq!(keyboard.key(), Key::Layer(Layer::Upper));
        assert_eq!(keyboard.handle(Activate, ""), Outcome::Layer);
        assert_eq!(keyboard.layer(), Layer::Upper);
        // Switching layers types nothing and hides nothing.
        assert!(keyboard.hidden());
    }

    #[test]
    fn the_cursor_moves_through_the_layer_it_shows() {
        // Up from the capitals' first key.
        let mut keyboard = passphrase_on(&[Down, Down, Down]);
        keyboard.handle(Activate, "");
        keyboard.step(Up);
        assert_eq!(keyboard.key(), Key::Char('Z'));
        keyboard.step(Up);
        assert_eq!(keyboard.key(), Key::Char('A'));
        assert_eq!(
            keyboard.handle(Activate, "pass"),
            Outcome::Edit("passA".to_owned())
        );
        // Down to the bottom row and on to the symbols, from its second key.
        let mut keyboard = passphrase_on(&[Down, Down, Down, Right]);
        keyboard.handle(Activate, "");
        assert_eq!(keyboard.layer(), Layer::Symbols);
        keyboard.step(Up);
        assert_eq!(keyboard.key(), Key::Char('.'));
        keyboard.step(Up);
        assert_eq!(keyboard.key(), Key::Char(';'));
        keyboard.step(Up);
        assert_eq!(keyboard.key(), Key::Char('='));
        keyboard.step(Up);
        assert_eq!(keyboard.key(), Key::Char('#'));
        // The symbols' second row is short: down from ")" lands on its last key.
        keyboard.step(Left);
        keyboard.step(Left);
        keyboard.step(Left);
        assert_eq!(keyboard.key(), Key::Char(')'));
        keyboard.step(Down);
        assert_eq!(keyboard.key(), Key::Char('}'));
        keyboard.step(Down);
        assert_eq!(keyboard.key(), Key::Char('`'));
        keyboard.step(Down);
        assert_eq!(keyboard.key(), Key::Backspace);
        keyboard.step(Down);
        assert_eq!(keyboard.key(), Key::Done);
        // Wrapping round the bottom row and the top of the grid.
        keyboard.step(Right);
        assert_eq!(keyboard.key(), Key::Layer(Layer::Upper));
        keyboard.step(Down);
        assert_eq!(keyboard.key(), Key::Char('!'));
    }

    #[test]
    fn show_reveals_the_hidden_text_and_hides_it_again() {
        let mut keyboard = passphrase_on(&[Down, Down, Down, Right, Right, Right]);
        assert_eq!(keyboard.key(), Key::Reveal);
        assert!(keyboard.hidden());
        assert_eq!(keyboard.handle(Activate, "secret"), Outcome::Reveal);
        assert!(!keyboard.hidden());
        assert_eq!(keyboard.handle(Activate, "secret"), Outcome::Reveal);
        assert!(keyboard.hidden());
        assert_eq!(keyboard.key(), Key::Reveal);
    }

    #[test]
    fn the_passphrase_keyboard_deletes_cancels_and_confirms_as_search_does() {
        let edit = |text: &str| Outcome::Edit(text.to_owned());
        let mut keyboard = Keyboard::passphrase();
        assert_eq!(keyboard.handle(Back, "ab"), edit("a"));
        assert_eq!(keyboard.handle(Back, ""), Outcome::Close);
        assert_eq!(keyboard.handle(Menu, "secret"), Outcome::Confirm);
        let mut done = passphrase_on(&[Up, Up, Left]);
        assert_eq!(done.key(), Key::Done);
        assert_eq!(done.handle(Activate, "secret"), Outcome::Confirm);
        let mut space = passphrase_on(&[Up, Up, Right, Right]);
        assert_eq!(space.key(), Key::Space);
        assert_eq!(space.handle(Activate, "a"), edit("a "));
    }

    /// Moves `keyboard` to a key that types `character`, switching layers as needed, by
    /// the fewest presses of the D-pad and A; `None` when no key types it.
    fn reach(keyboard: Keyboard, character: char) -> Option<Keyboard> {
        use std::collections::{HashSet, VecDeque};
        let types = |keyboard: &Keyboard| match keyboard.key() {
            Key::Char(c) => c == character,
            Key::Space => character == ' ',
            _ => false,
        };
        let mut seen = HashSet::from([keyboard]);
        let mut queue = VecDeque::from([keyboard]);
        while let Some(keyboard) = queue.pop_front() {
            if types(&keyboard) {
                return Some(keyboard);
            }
            // The D-pad moves the cursor, and A on a layer key switches layers.
            let switch = match keyboard.key() {
                Key::Layer(_) => Some(Activate),
                _ => None,
            };
            let intents = [Up, Down, Left, Right]
                .map(Navigate)
                .into_iter()
                .chain(switch);
            for intent in intents {
                let mut next = keyboard;
                next.handle(intent, "");
                if seen.insert(next) {
                    queue.push_back(next);
                }
            }
        }
        None
    }

    #[test]
    fn the_controller_alone_types_any_printable_ascii_passphrase() {
        let passphrase: String = (' '..='~').collect();
        let mut keyboard = Keyboard::passphrase();
        let mut text = String::new();
        for character in passphrase.chars() {
            keyboard =
                reach(keyboard, character).unwrap_or_else(|| panic!("no key types {character:?}"));
            match keyboard.handle(Activate, &text) {
                Outcome::Edit(edited) => text = edited,
                outcome => panic!("{character:?} gave {outcome:?}"),
            }
        }
        assert_eq!(text, passphrase);
        assert_eq!(keyboard.handle(Menu, &text), Outcome::Confirm);
    }
}
