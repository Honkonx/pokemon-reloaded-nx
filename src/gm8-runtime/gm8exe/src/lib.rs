#![cfg_attr(not(feature = "std"), no_std)]
#![cfg_attr(not(feature = "std"), feature(prelude_import))]
#![allow(clippy::cognitive_complexity)]
#![allow(clippy::unreadable_literal)]

#[cfg(not(feature = "std"))]
#[macro_use]
extern crate alloc;

// Without std the crate uses the core prelude plus the alloc types it relies on.
#[cfg(not(feature = "std"))]
#[allow(unused_imports)]
mod nostd_prelude {
    pub use alloc::{
        borrow::ToOwned,
        boxed::Box,
        string::{String, ToString},
        vec::Vec,
    };
    pub use core::prelude::rust_2021::*;
}
#[cfg(not(feature = "std"))]
#[prelude_import]
#[allow(unused_imports)]
use nostd_prelude::*;

macro_rules! log {
    ($logger: expr, $x: expr) => {
        if let Some(logger) = &$logger {
            logger($x.into());
        }
    };
    ($logger: expr, $format: expr, $($x: expr),*) => {
        if let Some(logger) = &$logger {
            logger(&format!(
                $format,
                $($x),*
            ));
        }
    };
    ($($x:expr,)*) => (log![$($x),*]); // leveraged from vec![]
}

pub mod asset;
pub mod def;
pub mod gamedata;
pub mod reader;
pub mod rsrc;
pub mod settings;
pub mod upx;

mod colour;

use crate::asset::*;
use settings::{GameHelpDialog, Settings};

pub type AssetList<T> = Vec<Option<Box<T>>>;

pub struct GameAssets {
    pub triggers: AssetList<Trigger>,
    pub constants: Vec<Constant>,
    pub extensions: Vec<Extension>,
    pub sprites: AssetList<Sprite>,
    pub sounds: AssetList<Sound>,
    pub backgrounds: AssetList<Background>,
    pub paths: AssetList<Path>,
    pub scripts: AssetList<Script>,
    pub fonts: AssetList<Font>,
    pub timelines: AssetList<Timeline>,
    pub objects: AssetList<Object>,
    pub rooms: AssetList<Room>,
    pub included_files: Vec<IncludedFile>,
    pub version: GameVersion,

    pub dx_dll: Vec<u8>,
    pub ico_file_raw: Option<Vec<u8>>,
    pub help_dialog: GameHelpDialog,
    pub last_instance_id: i32,
    pub last_tile_id: i32,
    pub library_init_strings: Vec<PascalString>,
    pub room_order: Vec<i32>,

    pub settings: Settings,
    pub game_id: u32,
    pub guid: [u32; 4],
}

#[derive(Copy, Clone, Debug)]
pub enum GameVersion {
    GameMaker8_0,
    GameMaker8_1,
}

pub use colour::Colour;
