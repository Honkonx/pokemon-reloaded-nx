#![no_std]

//! Bounded index and slice view over the complete standard GM8.0 resource directory.
//!
//! This crate connects the directory probe to a lifetime-safe asset lookup table.
//! It does not decompress resource records, parse unsupported asset families, or
//! execute GML. The game data remains borrowed from the caller-owned payload.

extern crate alloc;

use alloc::vec::Vec;
use gm8_format_probe::{
    AssetSpan, Gm80AssetKind, Gm80DirectoryError, Gm80DirectorySummary, Gm80IndexLimits, scan_gm80_resource_directory,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub directory: Gm80IndexLimits,
    pub max_indexed_assets: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            directory: Gm80IndexLimits {
                max_records: 262_144,
                max_pascal_string_bytes: 1024 * 1024,
                max_compressed_asset_bytes: 64 * 1024 * 1024,
            },
            max_indexed_assets: 65_536,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AssetReference {
    pub kind: Gm80AssetKind,
    pub index: u32,
    pub span: AssetSpan,
}

impl AssetReference {
    fn key(self) -> (u8, u32) {
        let kind = match self.kind {
            Gm80AssetKind::Trigger => 0,
            Gm80AssetKind::Sound => 1,
            Gm80AssetKind::Sprite => 2,
            Gm80AssetKind::Background => 3,
            Gm80AssetKind::Path => 4,
            Gm80AssetKind::Script => 5,
            Gm80AssetKind::Font => 6,
            Gm80AssetKind::Timeline => 7,
            Gm80AssetKind::Object => 8,
            Gm80AssetKind::Room => 9,
            Gm80AssetKind::IncludedFile => 10,
        };
        (kind, self.index)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadError {
    Directory(Gm80DirectoryError),
    TooManyIndexedAssets,
    InvalidAssetSpan,
}

/// An index of every compressed resource record in a standard GM8.0 payload.
///
/// The payload is borrowed, so index creation does not duplicate game data.
/// It uses at most `max_indexed_assets` references and preserves table indexes,
/// including deleted records, for later decoding by the corresponding parser.
pub struct Gm80AssetIndex<'a> {
    payload: &'a [u8],
    directory: Gm80DirectorySummary,
    assets: Vec<AssetReference>,
}

impl<'a> Gm80AssetIndex<'a> {
    pub fn load(payload: &'a [u8], limits: Limits) -> Result<Self, LoadError> {
        let mut assets = Vec::new();
        let mut exceeded = false;
        let directory = scan_gm80_resource_directory(payload, limits.directory, |kind, index, span| {
            if assets.len() >= limits.max_indexed_assets as usize {
                exceeded = true;
            } else {
                assets.push(AssetReference { kind, index, span });
            }
        })
        .map_err(LoadError::Directory)?;

        if exceeded {
            return Err(LoadError::TooManyIndexedAssets);
        }
        Ok(Self { payload, directory, assets })
    }

    pub fn directory(&self) -> Gm80DirectorySummary {
        self.directory
    }

    /// References in GM8.0 directory order, grouped by kind then index.
    pub fn assets(&self) -> &[AssetReference] {
        &self.assets
    }

    pub fn get(&self, kind: Gm80AssetKind, index: u32) -> Option<&'a [u8]> {
        let key = AssetReference { kind, index, span: AssetSpan::default() }.key();
        let position = self.assets.binary_search_by_key(&key, |asset| asset.key()).ok()?;
        let asset = &self.assets[position];
        let start = usize::try_from(asset.span.offset).ok()?;
        let length = usize::try_from(asset.span.length).ok()?;
        let end = start.checked_add(length)?;
        self.payload.get(start..end)
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use std::vec::Vec;

    fn push_u32(out: &mut Vec<u8>, value: u32) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    fn empty_payload_with_one_sprite() -> Vec<u8> {
        let mut out = Vec::new();
        push_u32(&mut out, 0); // random dword count
        push_u32(&mut out, 1); // Pro flag
        push_u32(&mut out, 0x1234_5678); // game id
        for value in [1, 2, 3, 4] {
            push_u32(&mut out, value);
        }
        push_u32(&mut out, 700); // extensions
        push_u32(&mut out, 0);
        push_u32(&mut out, 800); // triggers
        push_u32(&mut out, 1);
        push_u32(&mut out, 2);
        out.extend_from_slice(&[0x11, 0x22]);
        push_u32(&mut out, 800); // constants
        push_u32(&mut out, 0);
        push_u32(&mut out, 800); // sounds
        push_u32(&mut out, 0);
        push_u32(&mut out, 800); // sprites
        push_u32(&mut out, 1);
        push_u32(&mut out, 3);
        out.extend_from_slice(&[0xaa, 0xbb, 0xcc]);
        for _ in 0..7 {
            push_u32(&mut out, 800);
            push_u32(&mut out, 0);
        }
        push_u32(&mut out, 0); // last instance id
        push_u32(&mut out, 0); // last tile id
        push_u32(&mut out, 800); // included files
        push_u32(&mut out, 0);
        push_u32(&mut out, 800); // help data
        push_u32(&mut out, 0);
        push_u32(&mut out, 500); // init strings
        push_u32(&mut out, 0);
        push_u32(&mut out, 700); // room order
        push_u32(&mut out, 0);
        out
    }

    #[test]
    fn indexes_full_directory_and_returns_borrowed_record_bytes() {
        let payload = empty_payload_with_one_sprite();
        let index = Gm80AssetIndex::load(&payload, Limits::default()).unwrap();
        assert!(index.directory().prefix.pro_flag);
        assert_eq!(index.directory().prefix.game_id, 0x1234_5678);
        assert_eq!(index.directory().tail.sprite_count, 1);
        assert_eq!(index.assets().len(), 2);
        assert_eq!(index.assets()[0].kind, Gm80AssetKind::Trigger);
        assert_eq!(index.assets()[1].kind, Gm80AssetKind::Sprite);
        assert_eq!(index.get(Gm80AssetKind::Trigger, 0), Some(&[0x11, 0x22][..]));
        assert_eq!(index.get(Gm80AssetKind::Sprite, 0), Some(&[0xaa, 0xbb, 0xcc][..]));
        assert_eq!(index.get(Gm80AssetKind::Sprite, 1), None);
    }

    #[test]
    fn enforces_reference_count_limit_and_rejects_truncation() {
        let payload = empty_payload_with_one_sprite();
        let limits = Limits { max_indexed_assets: 1, ..Limits::default() };
        assert_eq!(Gm80AssetIndex::load(&payload, limits).err(), Some(LoadError::TooManyIndexedAssets));
        assert!(matches!(
            Gm80AssetIndex::load(&payload[..payload.len() - 1], Limits::default()),
            Err(LoadError::Directory(_))
        ));
    }
}
