"""Checks that every translation in po/*.po keeps the {name} placeholders of its source string.

The UI fills placeholders by replacing them in the translated text, for example
`gettext("Installing {store}…").replace("{store}", ...)`, so a translation that drops a
placeholder loses its value, and one that renames it shows the braces to the user.

Run with: python3 -m unittest discover -s tests/scripts
"""

import ast
import re
import unittest
from dataclasses import dataclass, field
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PLACEHOLDER = re.compile(r"\{[A-Za-z_][A-Za-z0-9_]*\}")
KEYWORD = re.compile(r"^(msgctxt|msgid_plural|msgid|msgstr(?:\[\d+\])?)\s+(\".*\")$")


@dataclass
class Entry:
    fields: dict[str, str] = field(default_factory=dict)
    fuzzy: bool = False


def entries(text: str) -> list[Entry]:
    """The live entries of a PO file; obsolete (#~) entries and the header are left out."""
    found: list[Entry] = []
    current, key = Entry(), None
    for line in [*text.splitlines(), ""]:
        if not line.strip():
            if current.fields.get("msgid"):
                found.append(current)
            current, key = Entry(), None
        elif line.startswith("#,"):
            current.fuzzy = current.fuzzy or "fuzzy" in {flag.strip() for flag in line[2:].split(",")}
        elif line.startswith("#"):
            continue
        elif match := KEYWORD.match(line):
            key = match.group(1)
            current.fields[key] = ast.literal_eval(match.group(2))
        elif line.startswith('"') and key is not None:
            current.fields[key] += ast.literal_eval(line)
    return found


def placeholders(text: str) -> set[str]:
    return set(PLACEHOLDER.findall(text))


def problems(text: str) -> list[str]:
    """Translated, non-fuzzy entries whose placeholders do not fit their source.

    A singular translation uses exactly the placeholders of its msgid. A plural form uses only
    placeholders of msgid or msgid_plural, and may leave the count out: Dutch writes the
    one-item form of "Added {count} games" as "Een game toegevoegd".
    """
    found = []
    for entry in entries(text):
        if entry.fuzzy:
            continue
        source = entry.fields["msgid"]
        if "msgid_plural" in entry.fields:
            allowed = placeholders(source) | placeholders(entry.fields["msgid_plural"])
            for key, translation in entry.fields.items():
                if key.startswith("msgstr[") and translation and not placeholders(translation) <= allowed:
                    found.append(f"{source!r} {key} {translation!r}")
        elif (translation := entry.fields.get("msgstr")) and placeholders(translation) != placeholders(source):
            found.append(f"{source!r} -> {translation!r}")
    return found


def po(body: str) -> str:
    return f'msgid ""\nmsgstr ""\n"Content-Type: text/plain; charset=UTF-8\\n"\n\n{body}'


class TranslationsTest(unittest.TestCase):
    def test_every_translation_keeps_the_placeholders_of_its_source(self) -> None:
        files = sorted((ROOT / "po").glob("*.po"))
        self.assertTrue(files, "no po/*.po files found")
        for path in files:
            with self.subTest(path.name):
                self.assertEqual(problems(path.read_text(encoding="utf-8")), [])

    def test_a_dropped_placeholder_is_reported(self) -> None:
        text = po('msgid "{store} Is Installed"\nmsgstr "Geïnstalleerd"\n')
        self.assertEqual(problems(text), ["'{store} Is Installed' -> 'Geïnstalleerd'"])

    def test_a_renamed_placeholder_is_reported(self) -> None:
        text = po('msgid "Installing {store}…"\nmsgstr "{winkel} wordt "\n"geïnstalleerd…"\n')
        self.assertEqual(problems(text), ["'Installing {store}…' -> '{winkel} wordt geïnstalleerd…'"])

    def test_placeholders_in_another_order_are_accepted(self) -> None:
        self.assertEqual(problems(po('msgid "{button}: {action}"\nmsgstr "{action}: {button}"\n')), [])

    def test_a_plural_form_may_leave_out_the_count(self) -> None:
        text = po(
            'msgid "Added one game"\nmsgid_plural "Added {count} games"\n'
            'msgstr[0] "Een game toegevoegd"\nmsgstr[1] "{count} games toegevoegd"\n'
        )
        self.assertEqual(problems(text), [])

    def test_a_plural_form_with_an_unknown_placeholder_is_reported(self) -> None:
        text = po(
            'msgid "Added one game"\nmsgid_plural "Added {count} games"\n'
            'msgstr[0] "Een game toegevoegd"\nmsgstr[1] "{aantal} games toegevoegd"\n'
        )
        self.assertEqual(problems(text), ["'Added one game' msgstr[1] '{aantal} games toegevoegd'"])

    def test_untranslated_fuzzy_and_obsolete_entries_are_skipped(self) -> None:
        text = po(
            'msgid "Join {network}"\nmsgstr ""\n\n'
            '#, fuzzy, rust-format\nmsgid "Forget {network}?"\nmsgstr "Vergeten?"\n\n'
            '#~ msgid "Open {launcher}"\n#~ msgstr "Openen"\n'
        )
        self.assertEqual(problems(text), [])


if __name__ == "__main__":
    unittest.main()
