#![no_std]

//! Bounded, indexed loading of GM8.0 Sprite resources.
//!
//! This is a vertical slice: it validates the standard resource directory,
//! inflates selected Sprite spans, and returns owned parsed records. It does
//! not construct OpenGMK's complete `GameAssets`, load other resource kinds,
//! create textures, or execute a game.

extern crate alloc;

use alloc::vec::Vec;
use gm8_assets::{AssetReference, Gm80AssetIndex, Limits as IndexLimits, LoadError as IndexError};
use gm8_background::{
    Background, Limits as BackgroundLimits, ParseError as BackgroundParseError, parse_compressed_background_record,
};
use gm8_format_probe::{Gm80AssetKind, Gm80DirectorySummary, Gm80IndexLimits};
use gm8_sprite::{Limits as SpriteLimits, ParseError as SpriteParseError, Sprite, parse_compressed_sprite_record};
use gm8_zlib::InflateError;

/// Budgets applied while indexing and decoding the supported Sprite records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub directory: Gm80IndexLimits,
    pub max_indexed_assets: u32,
    pub max_sprite_records: u32,
    pub max_background_records: u32,
    /// Maximum combined live owned payload bytes. During parsing, half the
    /// remaining budget caps the transient inflated record, leaving the other
    /// half for its copied fields. This does not cap allocator metadata or
    /// excess `Vec` capacity.
    pub max_total_owned_bytes: usize,
    pub sprite: SpriteLimits,
    pub background: BackgroundLimits,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            directory: Gm80IndexLimits {
                max_records: 262_144,
                max_pascal_string_bytes: 1024 * 1024,
                max_compressed_asset_bytes: 8 * 1024 * 1024,
            },
            max_indexed_assets: 65_536,
            max_sprite_records: 16_384,
            max_background_records: 16_384,
            max_total_owned_bytes: 128 * 1024 * 1024,
            sprite: SpriteLimits::default(),
            background: BackgroundLimits::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnedSpriteRecord {
    pub index: u32,
    pub sprite: Sprite,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpriteResources {
    pub directory: Gm80DirectorySummary,
    pub records: Vec<OwnedSpriteRecord>,
    pub deleted_records: u32,
    pub owned_bytes: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnedBackgroundRecord {
    pub index: u32,
    pub background: Background,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackgroundResources {
    pub directory: Gm80DirectorySummary,
    pub records: Vec<OwnedBackgroundRecord>,
    pub deleted_records: u32,
    pub owned_bytes: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadError {
    Index(IndexError),
    TooManySpriteRecords,
    TooManyBackgroundRecords,
    Parse { index: u32, error: SpriteParseError },
    BackgroundParse { index: u32, error: BackgroundParseError },
    OwnedByteBudgetExceeded,
}

/// Load every non-deleted Sprite from a standard GM8.0 payload.
///
/// Resource bytes are obtained through `Gm80AssetIndex`; each compressed
/// record is inflated under the smaller of the parser cap and remaining total
/// budget. Unknown resource families remain indexed but are not decoded.
pub fn load_gm80_sprite_resources(payload: &[u8], limits: Limits) -> Result<SpriteResources, LoadError> {
    let index = index_payload(payload, limits)?;
    let sprite_refs: Vec<AssetReference> =
        index.assets().iter().copied().filter(|asset| asset.kind == Gm80AssetKind::Sprite).collect();
    if sprite_refs.len() > limits.max_sprite_records as usize {
        return Err(LoadError::TooManySpriteRecords);
    }

    let mut records = Vec::new();
    records.try_reserve_exact(sprite_refs.len()).map_err(|_| LoadError::TooManySpriteRecords)?;
    let mut deleted_records = 0u32;
    let mut owned_bytes = 0usize;
    for reference in sprite_refs {
        let compressed = index
            .get(reference.kind, reference.index)
            .ok_or_else(|| LoadError::Parse { index: reference.index, error: SpriteParseError::Truncated })?;
        let remaining = limits.max_total_owned_bytes.saturating_sub(owned_bytes);
        if remaining == 0 {
            return Err(LoadError::OwnedByteBudgetExceeded);
        }
        let transient_cap = remaining / 2;
        if transient_cap == 0 {
            return Err(LoadError::OwnedByteBudgetExceeded);
        }
        let mut record_limits = limits.sprite;
        record_limits.max_decompressed_bytes = record_limits.max_decompressed_bytes.min(transient_cap);
        let sprite = match parse_compressed_sprite_record(compressed, record_limits) {
            Ok(sprite) => sprite,
            Err(SpriteParseError::DeletedAsset) => {
                deleted_records = deleted_records.saturating_add(1);
                continue;
            },
            Err(SpriteParseError::Inflate(InflateError::OutputLimitExceeded))
                if transient_cap < limits.sprite.max_decompressed_bytes =>
            {
                return Err(LoadError::OwnedByteBudgetExceeded);
            },
            Err(error) => return Err(LoadError::Parse { index: reference.index, error }),
        };
        let record_bytes = sprite_owned_bytes(&sprite).ok_or(LoadError::OwnedByteBudgetExceeded)?;
        owned_bytes = owned_bytes.checked_add(record_bytes).ok_or(LoadError::OwnedByteBudgetExceeded)?;
        if owned_bytes > limits.max_total_owned_bytes {
            return Err(LoadError::OwnedByteBudgetExceeded);
        }
        records.push(OwnedSpriteRecord { index: reference.index, sprite });
    }

    Ok(SpriteResources { directory: index.directory(), records, deleted_records, owned_bytes })
}

/// Load every non-deleted GM8.0 Background through the shared indexed spans.
/// The same aggregate budget is enforced for this table as for sprites.
pub fn load_gm80_background_resources(payload: &[u8], limits: Limits) -> Result<BackgroundResources, LoadError> {
    let index = index_payload(payload, limits)?;
    let references: Vec<AssetReference> =
        index.assets().iter().copied().filter(|asset| asset.kind == Gm80AssetKind::Background).collect();
    if references.len() > limits.max_background_records as usize {
        return Err(LoadError::TooManyBackgroundRecords);
    }

    let mut records = Vec::new();
    records.try_reserve_exact(references.len()).map_err(|_| LoadError::TooManyBackgroundRecords)?;
    let mut deleted_records = 0u32;
    let mut owned_bytes = 0usize;
    for reference in references {
        let compressed = index.get(reference.kind, reference.index).ok_or_else(|| LoadError::BackgroundParse {
            index: reference.index,
            error: BackgroundParseError::Truncated,
        })?;
        let remaining = limits.max_total_owned_bytes.saturating_sub(owned_bytes);
        if remaining == 0 {
            return Err(LoadError::OwnedByteBudgetExceeded);
        }
        let transient_cap = remaining / 2;
        if transient_cap == 0 {
            return Err(LoadError::OwnedByteBudgetExceeded);
        }
        let mut record_limits = limits.background;
        record_limits.max_decompressed_bytes = record_limits.max_decompressed_bytes.min(transient_cap);
        let background = match parse_compressed_background_record(compressed, record_limits) {
            Ok(background) => background,
            Err(BackgroundParseError::DeletedAsset) => {
                deleted_records = deleted_records.saturating_add(1);
                continue;
            },
            Err(BackgroundParseError::Inflate(InflateError::OutputLimitExceeded))
                if transient_cap < limits.background.max_decompressed_bytes =>
            {
                return Err(LoadError::OwnedByteBudgetExceeded);
            },
            Err(error) => return Err(LoadError::BackgroundParse { index: reference.index, error }),
        };
        let record_bytes = background
            .name
            .len()
            .checked_add(background.bgra.as_ref().map_or(0, Vec::len))
            .ok_or(LoadError::OwnedByteBudgetExceeded)?;
        owned_bytes = owned_bytes.checked_add(record_bytes).ok_or(LoadError::OwnedByteBudgetExceeded)?;
        if owned_bytes > limits.max_total_owned_bytes {
            return Err(LoadError::OwnedByteBudgetExceeded);
        }
        records.push(OwnedBackgroundRecord { index: reference.index, background });
    }
    Ok(BackgroundResources { directory: index.directory(), records, deleted_records, owned_bytes })
}

fn index_payload(payload: &[u8], limits: Limits) -> Result<Gm80AssetIndex<'_>, LoadError> {
    let index_limits = IndexLimits { directory: limits.directory, max_indexed_assets: limits.max_indexed_assets };
    Gm80AssetIndex::load(payload, index_limits).map_err(LoadError::Index)
}

fn sprite_owned_bytes(sprite: &Sprite) -> Option<usize> {
    let mut total = sprite.name.len();
    for frame in &sprite.frames {
        total = total.checked_add(frame.rgba.len())?;
    }
    for collider in &sprite.colliders {
        total = total.checked_add(collider.data.len())?;
    }
    Some(total)
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use gm8_format_probe::{Gm80DirectoryError, IndexError as DirectoryIndexError};
    use std::vec::Vec;

    fn u32le(out: &mut Vec<u8>, n: u32) {
        out.extend_from_slice(&n.to_le_bytes());
    }

    fn sprite_record(name: &[u8], rgba: &[u8]) -> Vec<u8> {
        let mut raw = Vec::new();
        u32le(&mut raw, 1);
        u32le(&mut raw, name.len() as u32);
        raw.extend_from_slice(name);
        u32le(&mut raw, 800); // Sprite record version
        u32le(&mut raw, 0); // origin x
        u32le(&mut raw, 0); // origin y
        u32le(&mut raw, 1); // frame count
        u32le(&mut raw, 800); // frame version
        u32le(&mut raw, 1); // width
        u32le(&mut raw, 1); // height
        u32le(&mut raw, rgba.len() as u32);
        raw.extend_from_slice(rgba);
        u32le(&mut raw, 0); // shared collision map
        u32le(&mut raw, 800); // collision version
        u32le(&mut raw, 1); // width
        u32le(&mut raw, 1); // height
        for _ in 0..4 {
            u32le(&mut raw, 0);
        } // bbox
        u32le(&mut raw, 1); // occupied collision pixel
        miniz_oxide::deflate::compress_to_vec_zlib(&raw, 6)
    }

    fn deleted_record() -> Vec<u8> {
        miniz_oxide::deflate::compress_to_vec_zlib(&0u32.to_le_bytes(), 6)
    }

    fn payload(sprite_records: &[Vec<u8>], background_records: &[Vec<u8>]) -> Vec<u8> {
        let mut out = Vec::new();
        u32le(&mut out, 0); // random dword count
        u32le(&mut out, 1); // pro flag
        u32le(&mut out, 42); // game id
        for _ in 0..4 {
            u32le(&mut out, 0);
        } // GUID
        u32le(&mut out, 700);
        u32le(&mut out, 0); // extensions
        u32le(&mut out, 800);
        u32le(&mut out, 0); // triggers
        u32le(&mut out, 800);
        u32le(&mut out, 0); // constants
        u32le(&mut out, 800);
        u32le(&mut out, 0); // sounds
        u32le(&mut out, 800);
        u32le(&mut out, sprite_records.len() as u32);
        for record in sprite_records {
            u32le(&mut out, record.len() as u32);
            out.extend_from_slice(record);
        }
        u32le(&mut out, 800); // backgrounds
        u32le(&mut out, background_records.len() as u32);
        for record in background_records {
            u32le(&mut out, record.len() as u32);
            out.extend_from_slice(record);
        }
        for _ in 0..6 {
            u32le(&mut out, 800);
            u32le(&mut out, 0);
        }
        u32le(&mut out, 0);
        u32le(&mut out, 0); // last instance/tile ids
        u32le(&mut out, 800);
        u32le(&mut out, 0); // included files
        u32le(&mut out, 800);
        u32le(&mut out, 0); // help
        u32le(&mut out, 500);
        u32le(&mut out, 0); // init strings
        u32le(&mut out, 700);
        u32le(&mut out, 0); // room order
        out
    }

    fn small_limits() -> Limits {
        Limits {
            directory: Gm80IndexLimits {
                max_records: 64,
                max_pascal_string_bytes: 1024,
                max_compressed_asset_bytes: 4096,
            },
            max_indexed_assets: 64,
            max_sprite_records: 8,
            max_background_records: 8,
            max_total_owned_bytes: 4096,
            sprite: SpriteLimits {
                max_compressed_bytes: 4096,
                max_decompressed_bytes: 4096,
                max_name_bytes: 64,
                max_frames: 4,
                max_width: 8,
                max_height: 8,
                max_frame_bytes: 256,
                max_collision_pixels: 64,
            },
            background: BackgroundLimits {
                max_compressed_bytes: 4096,
                max_decompressed_bytes: 4096,
                max_name_bytes: 64,
                max_pixel_bytes: 256,
                max_width: 8,
                max_height: 8,
            },
        }
    }

    fn background_record(name: &[u8], bgra: &[u8]) -> Vec<u8> {
        let mut raw = Vec::new();
        u32le(&mut raw, 1);
        u32le(&mut raw, name.len() as u32);
        raw.extend_from_slice(name);
        u32le(&mut raw, 710);
        u32le(&mut raw, 800);
        u32le(&mut raw, 1);
        u32le(&mut raw, 1);
        u32le(&mut raw, bgra.len() as u32);
        raw.extend_from_slice(bgra);
        miniz_oxide::deflate::compress_to_vec_zlib(&raw, 6)
    }

    #[test]
    fn inflates_indexed_sprite_to_owned_rgba_record() {
        let data = payload(&[sprite_record(b"synthetic", &[10, 20, 30, 255])], &[]);
        let loaded = load_gm80_sprite_resources(&data, small_limits()).unwrap();
        assert_eq!(loaded.directory.tail.sprite_count, 1);
        assert_eq!(loaded.records.len(), 1);
        assert_eq!(loaded.records[0].index, 0);
        assert_eq!(loaded.records[0].sprite.name, b"synthetic");
        assert_eq!(loaded.records[0].sprite.frames[0].rgba, [10, 20, 30, 255]);
        assert_eq!(loaded.owned_bytes, b"synthetic".len() + 4 + 1);
    }

    #[test]
    fn preserves_deleted_indexes_without_returning_them_as_resources() {
        let data = payload(&[deleted_record(), sprite_record(b"live", &[1, 2, 3, 4])], &[]);
        let loaded = load_gm80_sprite_resources(&data, small_limits()).unwrap();
        assert_eq!(loaded.deleted_records, 1);
        assert_eq!(loaded.records.len(), 1);
        assert_eq!(loaded.records[0].index, 1);
    }

    #[test]
    fn applies_aggregate_output_budget_across_records() {
        let data = payload(&[sprite_record(b"one", &[1, 2, 3, 4]), sprite_record(b"two", &[5, 6, 7, 8])], &[]);
        let limits = Limits { max_total_owned_bytes: 10, ..small_limits() };
        assert_eq!(load_gm80_sprite_resources(&data, limits).err(), Some(LoadError::OwnedByteBudgetExceeded));
    }

    #[test]
    fn rejects_truncated_directory_and_record_count_over_budget() {
        let data = payload(&[sprite_record(b"one", &[1, 2, 3, 4])], &[]);
        assert!(matches!(
            load_gm80_sprite_resources(&data[..data.len() - 1], small_limits()),
            Err(LoadError::Index(IndexError::Directory(Gm80DirectoryError::Tail(DirectoryIndexError::Truncated))))
        ));
        let limits = Limits { max_sprite_records: 0, ..small_limits() };
        assert_eq!(load_gm80_sprite_resources(&data, limits).err(), Some(LoadError::TooManySpriteRecords));
    }

    #[test]
    fn loads_indexed_background_and_preserves_original_index() {
        let data = payload(&[], &[deleted_record(), background_record(b"forest", &[3, 2, 1, 255])]);
        let loaded = load_gm80_background_resources(&data, small_limits()).unwrap();
        assert_eq!(loaded.directory.tail.background_count, 2);
        assert_eq!(loaded.deleted_records, 1);
        assert_eq!(loaded.records.len(), 1);
        assert_eq!(loaded.records[0].index, 1);
        assert_eq!(loaded.records[0].background.name, b"forest");
        assert_eq!(loaded.records[0].background.bgra.as_deref(), Some(&[3, 2, 1, 255][..]));
        assert_eq!(loaded.owned_bytes, b"forest".len() + 4);
    }

    #[test]
    fn enforces_background_aggregate_budget_and_record_count() {
        let data = payload(&[], &[background_record(b"one", &[1, 2, 3, 4]), background_record(b"two", &[5, 6, 7, 8])]);
        let limits = Limits { max_total_owned_bytes: 10, ..small_limits() };
        assert_eq!(load_gm80_background_resources(&data, limits).err(), Some(LoadError::OwnedByteBudgetExceeded));
        let limits = Limits { max_background_records: 1, ..small_limits() };
        assert_eq!(load_gm80_background_resources(&data, limits).err(), Some(LoadError::TooManyBackgroundRecords));
    }
}
