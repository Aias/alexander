//! What an exported file was made from: the original's path and the look, as JSON, kept in
//! the file itself (a PNG iTXt chunk, a JPEG APP11 segment). Opening such a file brings
//! back the original with its look, so a picture is never processed twice.

use crate::params::Settings;
use serde::{Deserialize, Serialize};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

pub const KEY: &str = "alexander-studio";
/// Marks our JPEG segment among other APP11 users (JPEG XT uses it too).
pub const JPEG_TAG: &[u8] = b"alexander-studio\0";
pub const JPEG_APP: u8 = 11;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub source: PathBuf,
    pub look: Settings,
}

impl Record {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("record")
    }

    pub fn from_json(s: &str) -> Option<Record> {
        serde_json::from_str(s).ok()
    }
}

/// The record in `path`, if it has one.
pub fn read(path: &Path) -> Option<Record> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut sig = [0u8; 8];
    f.read_exact(&mut sig).ok()?;
    f.seek(SeekFrom::Start(0)).ok()?;
    if sig == [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a] {
        read_png(f)
    } else if sig[..2] == [0xff, 0xd8] {
        read_jpeg(std::io::BufReader::new(f))
    } else {
        None
    }
}

fn read_png(f: std::fs::File) -> Option<Record> {
    let reader = png::Decoder::new(std::io::BufReader::new(f)).read_info().ok()?;
    let info = reader.info();
    for t in &info.utf8_text {
        if t.keyword == KEY {
            return Record::from_json(&t.get_text().ok()?);
        }
    }
    for t in &info.uncompressed_latin1_text {
        if t.keyword == KEY {
            return Record::from_json(&t.text);
        }
    }
    None
}

/// Walk the segments before the image data, looking for ours.
fn read_jpeg(mut f: impl Read) -> Option<Record> {
    let mut b = [0u8; 2];
    f.read_exact(&mut b).ok()?;
    loop {
        f.read_exact(&mut b).ok()?;
        if b[0] != 0xff {
            return None;
        }
        let marker = b[1];
        if marker == 0xda || marker == 0xd9 {
            return None;
        }
        if (0xd0..=0xd7).contains(&marker) || marker == 0x01 || marker == 0xff {
            continue;
        }
        f.read_exact(&mut b).ok()?;
        let len = u16::from_be_bytes(b) as usize;
        let mut data = vec![0u8; len.checked_sub(2)?];
        f.read_exact(&mut data).ok()?;
        if marker == 0xe0 + JPEG_APP && data.starts_with(JPEG_TAG) {
            return Record::from_json(std::str::from_utf8(&data[JPEG_TAG.len()..]).ok()?);
        }
    }
}

/// The original a file should be opened as, with its look: the recorded source when the
/// file is an export of ours and that source is still there.
pub fn resolve(path: &Path) -> Option<Record> {
    let r = read(path)?;
    (r.source != path && r.source.is_file()).then_some(r)
}
