//! What a Windows program says about itself: the product name in its version resource and
//! its icon, read from the PE file's resource section by hand. Only the headers and the
//! resource section are read, so a large game costs no more than a small one.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::ops::Range;
use std::path::Path;

/// Resource types: one image of an icon, an icon's list of images, version information.
const RT_ICON: u32 = 3;
const RT_GROUP_ICON: u32 = 14;
const RT_VERSION: u32 = 16;
/// The start of a PNG file, which large icon images are.
const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
/// How much of the file start is read for the headers and the section table.
const HEADER_BYTES: u64 = 64 * 1024;
/// The largest resource section read; icons and version information are far smaller.
const MAX_RESOURCE_BYTES: u32 = 64 * 1024 * 1024;

/// A Windows program's own title and icon.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExeInfo {
    /// The product name, or else the file description, from its version resource.
    pub title: Option<String>,
    pub icon: Option<Icon>,
}

/// An icon from a Windows program, as an image file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Icon {
    pub bytes: Vec<u8>,
    /// `png` for the PNG images of large icons, else `ico`.
    pub extension: &'static str,
}

/// Reads a Windows program's title and icon; empty for files that are not PE programs or
/// have neither.
pub fn read_exe(path: &Path) -> ExeInfo {
    let Some(resources) = Resources::read(path) else {
        return ExeInfo::default();
    };
    ExeInfo {
        title: resources.title(),
        icon: resources.icon(),
    }
}

/// Little-endian reads that fail instead of panicking on short data.
fn u16_at(data: &[u8], offset: usize) -> Option<u16> {
    let bytes = data.get(offset..offset.checked_add(2)?)?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn u32_at(data: &[u8], offset: usize) -> Option<u32> {
    let bytes = data.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// The section that holds a program's resources.
struct Resources {
    /// The whole section, as stored in the file.
    section: Vec<u8>,
    /// Where the section is loaded, which data entries count their addresses from.
    section_rva: u32,
    /// Where the resource directory starts in the section.
    root: usize,
}

impl Resources {
    /// Finds the resource section through the PE headers and reads it.
    fn read(path: &Path) -> Option<Self> {
        let mut file = File::open(path).ok()?;
        let mut header = Vec::new();
        (&mut file)
            .take(HEADER_BYTES)
            .read_to_end(&mut header)
            .ok()?;
        if header.get(..2)? != b"MZ" {
            return None;
        }
        let pe = u32_at(&header, 0x3c)? as usize;
        if header.get(pe..pe.checked_add(4)?)? != b"PE\0\0" {
            return None;
        }
        let coff = pe + 4;
        let sections = u16_at(&header, coff + 2)? as usize;
        let optional_size = u16_at(&header, coff + 16)? as usize;
        let optional = coff + 20;
        // The data directories follow the fields of PE32 or of PE32+ (64-bit) programs.
        let directories = match u16_at(&header, optional)? {
            0x10b => optional + 96,
            0x20b => optional + 112,
            _ => return None,
        };
        if u32_at(&header, directories - 4)? <= 2 {
            return None;
        }
        // The third data directory is the resource directory.
        let rva = u32_at(&header, directories + 16)?;
        if rva == 0 {
            return None;
        }
        let table = optional + optional_size;
        (0..sections).find_map(|index| {
            let entry = table + 40 * index;
            let virtual_size = u32_at(&header, entry + 8)?;
            let section_rva = u32_at(&header, entry + 12)?;
            let raw_size = u32_at(&header, entry + 16)?;
            let raw_offset = u32_at(&header, entry + 20)?;
            let end = section_rva.checked_add(virtual_size.max(raw_size))?;
            if !(section_rva..end).contains(&rva) || raw_size > MAX_RESOURCE_BYTES {
                return None;
            }
            let mut section = Vec::new();
            file.seek(SeekFrom::Start(raw_offset.into())).ok()?;
            (&mut file)
                .take(raw_size.into())
                .read_to_end(&mut section)
                .ok()?;
            Some(Self {
                section,
                section_rva,
                root: (rva - section_rva) as usize,
            })
        })
    }

    /// The `(ID, offset)` entries of the resource directory at `offset` from the root;
    /// named entries have no ID. Offsets keep their "is a directory" bit.
    fn entries(&self, offset: u32) -> Vec<(Option<u32>, u32)> {
        let directory = self.root + (offset & 0x7fff_ffff) as usize;
        let count = |at| u16_at(&self.section, directory + at).unwrap_or(0) as usize;
        let named = count(12);
        (0..named + count(14))
            .map_while(|index| {
                let entry = directory + 16 + 8 * index;
                let name = u32_at(&self.section, entry)?;
                let target = u32_at(&self.section, entry + 4)?;
                Some(((index >= named).then_some(name), target))
            })
            .collect()
    }

    /// The data of a resource of type `kind`: the one with ID `id`, or the first one, in
    /// its first language.
    fn find(&self, kind: u32, id: Option<u32>) -> Option<&[u8]> {
        let is_directory = |target: u32| target & 0x8000_0000 != 0;
        let (_, names) = self
            .entries(0)
            .into_iter()
            .find(|&(entry, target)| entry == Some(kind) && is_directory(target))?;
        let (_, languages) = self
            .entries(names)
            .into_iter()
            .find(|&(entry, target)| is_directory(target) && (id.is_none() || entry == id))?;
        let (_, leaf) = self
            .entries(languages)
            .into_iter()
            .find(|&(_, target)| !is_directory(target))?;
        let leaf = self.root + leaf as usize;
        let rva = u32_at(&self.section, leaf)?;
        let size = u32_at(&self.section, leaf + 4)? as usize;
        let start = rva.checked_sub(self.section_rva)? as usize;
        self.section.get(start..start.checked_add(size)?)
    }

    /// The product name from the version resource, or else the file description; from
    /// the US English string table first, then the others.
    fn title(&self) -> Option<String> {
        let version = self.find(RT_VERSION, None)?;
        let root = Block::read(version, 0)?;
        let mut tables: Vec<Block> = root
            .children(version)
            .into_iter()
            .filter(|block| block.key == "StringFileInfo")
            .flat_map(|block| block.children(version))
            .collect();
        tables.sort_by_key(|table| !table.key.starts_with("0409"));
        ["ProductName", "FileDescription"].iter().find_map(|name| {
            tables.iter().find_map(|table| {
                table
                    .children(version)
                    .into_iter()
                    .find(|string| string.key == *name)
                    .map(|string| string.text(version))
                    .filter(|text| !text.is_empty())
            })
        })
    }

    /// The largest image of the first icon group, which Windows shows for the program:
    /// as a PNG when stored as one, else as a one-image `.ico` file.
    fn icon(&self) -> Option<Icon> {
        let group = self.find(RT_GROUP_ICON, None)?;
        let count = u16_at(group, 4)? as usize;
        let entry = (0..count)
            .filter_map(|index| group.get(6 + 14 * index..6 + 14 * (index + 1)))
            // A width of 0 means 256 pixels; more colours win among the same size.
            .max_by_key(|entry| {
                let width = if entry[0] == 0 { 256 } else { entry[0] as u16 };
                (width, u16_at(entry, 6).unwrap_or(0))
            })?;
        let image = self.find(RT_ICON, Some(u16_at(entry, 12)?.into()))?;
        if image.starts_with(PNG_SIGNATURE) {
            return Some(Icon {
                bytes: image.to_vec(),
                extension: "png",
            });
        }
        // An .ico file: its header, then the group's entry with the image's place in the
        // file instead of its resource ID.
        let mut bytes = vec![0, 0, 1, 0, 1, 0];
        bytes.extend_from_slice(&entry[..8]);
        bytes.extend_from_slice(&(image.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&22u32.to_le_bytes());
        bytes.extend_from_slice(image);
        Some(Icon {
            bytes,
            extension: "ico",
        })
    }
}

/// One block of a version resource: a length, a key, a value and child blocks.
struct Block {
    key: String,
    value: usize,
    children: Range<usize>,
}

impl Block {
    fn read(data: &[u8], offset: usize) -> Option<Self> {
        let length = u16_at(data, offset)? as usize;
        let value_length = u16_at(data, offset + 2)? as usize;
        let is_text = u16_at(data, offset + 4)? == 1;
        let end = offset.checked_add(length)?;
        if length < 6 || end > data.len() {
            return None;
        }
        let (key, after_key) = utf16_until_nul(data, offset + 6, end);
        let value = align4(after_key);
        let value_bytes = if is_text {
            value_length * 2
        } else {
            value_length
        };
        Some(Self {
            key,
            value,
            children: align4(value + value_bytes).min(end)..end,
        })
    }

    fn children(&self, data: &[u8]) -> Vec<Self> {
        let mut children = Vec::new();
        let mut offset = self.children.start;
        while offset + 6 <= self.children.end {
            let Some(length) = u16_at(data, offset).filter(|&length| length > 0) else {
                break;
            };
            let Some(child) = Self::read(data, offset) else {
                break;
            };
            children.push(child);
            offset = align4(offset + length as usize);
        }
        children
    }

    /// The value as text, up to its nul. Read to the block's end, since some programs
    /// give text lengths in bytes instead of characters.
    fn text(&self, data: &[u8]) -> String {
        let (text, _) = utf16_until_nul(data, self.value, self.children.end);
        text.trim().to_owned()
    }
}

/// The UTF-16 text from `start` up to a nul or `end`, and the offset after the nul.
fn utf16_until_nul(data: &[u8], start: usize, end: usize) -> (String, usize) {
    let mut units = Vec::new();
    let mut offset = start;
    while offset + 2 <= end {
        let Some(unit) = u16_at(data, offset) else {
            break;
        };
        offset += 2;
        if unit == 0 {
            break;
        }
        units.push(unit);
    }
    (String::from_utf16_lossy(&units), offset)
}

fn align4(offset: usize) -> usize {
    offset.next_multiple_of(4)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/pe")
            .join(name)
    }

    #[test]
    fn the_title_is_the_product_name() {
        assert_eq!(
            read_exe(&fixture("game.exe")).title.as_deref(),
            Some("Celeste")
        );
    }

    #[test]
    fn without_a_product_name_the_title_is_the_description_in_any_language() {
        assert_eq!(
            read_exe(&fixture("setup.exe")).title.as_deref(),
            Some("Celeste Setup")
        );
    }

    #[test]
    fn the_icon_is_the_largest_image_of_the_first_icon_group() {
        let icon = read_exe(&fixture("game.exe")).icon.unwrap();

        // The 256×256 image, which large icons keep as a PNG file.
        assert_eq!(icon.extension, "png");
        assert!(icon.bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert_eq!(&icon.bytes[16..24], [0, 0, 1, 0, 0, 0, 1, 0]);
    }

    #[test]
    fn a_bitmap_icon_becomes_an_ico_file_of_its_own() {
        let icon = read_exe(&fixture("setup.exe")).icon.unwrap();

        assert_eq!(icon.extension, "ico");
        // One 16×16 image of 32 bits, right after the 22 header bytes.
        assert_eq!(&icon.bytes[..6], [0, 0, 1, 0, 1, 0]);
        assert_eq!(&icon.bytes[6..8], [16, 16]);
        assert_eq!(u16_at(&icon.bytes, 12), Some(32));
        assert_eq!(u32_at(&icon.bytes, 18), Some(22));
        let image = u32_at(&icon.bytes, 14).unwrap() as usize;
        assert_eq!(icon.bytes.len(), 22 + image);
        // The image is the bitmap's header (40 bytes), its pixels and its mask.
        assert_eq!(u32_at(&icon.bytes, 22), Some(40));
        assert_eq!(image, 40 + 16 * 16 * 4 + 16 * 4);
    }

    #[test]
    fn programs_without_resources_and_other_files_say_nothing() {
        assert_eq!(read_exe(&fixture("plain.exe")), ExeInfo::default());
        assert_eq!(read_exe(&fixture("make_fixtures.py")), ExeInfo::default());
        assert_eq!(read_exe(&fixture("missing.exe")), ExeInfo::default());
    }
}
