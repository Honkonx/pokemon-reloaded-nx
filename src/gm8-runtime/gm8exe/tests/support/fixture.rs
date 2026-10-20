// SPDX-License-Identifier: GPL-2.0-only
// Original synthetic data. No game data or runnable Windows loader is included.
use gm8exe::{
    Colour, GameVersion,
    asset::{self, Asset, CodeAction},
};
use miniz_oxide::deflate::compress_to_vec_zlib;

pub const CREATE: &str = "
ticks = 0; sound_background_tempo(1); sound_play(snd_fixture);
var f;
f = file_bin_open('data/fixture-save.bin', 1);
file_bin_write_byte(f, 99);
file_bin_rewrite(f);
file_bin_write_byte(f, 42);
file_bin_close(f);
ini_open('data/fixture.ini');
ini_write_real('save', 'counter', 1);
ini_close();
ini_open('data/fixture.ini');
ini_write_real('save', 'counter', 2);
ini_close();
game_save('data/fixture-state.bin');
";
pub const STEP: &str = "
ticks += 1;
x = fixture_clamp(x + 4 * (keyboard_check(vk_right) - keyboard_check(vk_left)), 16, room_width - 16);
y = fixture_clamp(y + 4 * (keyboard_check(vk_down) - keyboard_check(vk_up)), 16, room_height - 16);
if (keyboard_check_pressed(ord('Z'))) sound_play(snd_fixture);
";
pub const DRAW: &str = "
draw_sprite(spr_fixture, 0, x, y);
draw_set_color(c_white);
draw_text(24, 24, 'PokeNX synthetic GM8 fixture');
draw_text(24, 48, 'Directions: move / A (Z): tone');
draw_text(24, 72, string(ticks));
";

/// Original silent MPEG-1 Layer III frames, wrapped as a WAVE with odd-sized
/// metadata before fmt. No encoder, external sample or copyrighted audio.
pub fn mp3_wave_with_metadata() -> Vec<u8> {
    fn chunk(out: &mut Vec<u8>, name: &[u8; 4], bytes: &[u8]) {
        out.extend_from_slice(name);
        out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(bytes);
        if bytes.len() & 1 != 0 {
            out.push(0);
        }
    }
    let mut out = b"RIFF\0\0\0\0WAVE".to_vec();
    chunk(&mut out, b"JUNK", &[0x55, 0, 7]);
    let mut fmt = Vec::new();
    fmt.extend_from_slice(&0x55u16.to_le_bytes());
    fmt.extend_from_slice(&2u16.to_le_bytes());
    fmt.extend_from_slice(&44_100u32.to_le_bytes());
    fmt.extend_from_slice(&16_000u32.to_le_bytes());
    for value in [1u16, 0, 12, 1] {
        fmt.extend_from_slice(&value.to_le_bytes());
    }
    fmt.extend_from_slice(&2u32.to_le_bytes());
    for value in [417u16, 1, 0] {
        fmt.extend_from_slice(&value.to_le_bytes());
    }
    chunk(&mut out, b"fmt ", &fmt);
    chunk(&mut out, b"fact", &3456u32.to_le_bytes());
    let mut frames = vec![0u8; 417 * 3];
    for frame in frames.chunks_exact_mut(417) {
        frame[..4].copy_from_slice(&[0xff, 0xfb, 0x90, 0]);
    }
    chunk(&mut out, b"data", &frames);
    let size = (out.len() - 8) as u32;
    out[4..8].copy_from_slice(&size.to_le_bytes());
    out
}

fn word(out: &mut Vec<u8>, n: u32) {
    out.extend_from_slice(&n.to_le_bytes());
}
fn string(out: &mut Vec<u8>, s: &str) {
    word(out, s.len() as u32);
    out.extend_from_slice(s.as_bytes());
}
fn chunk(out: &mut Vec<u8>, bytes: &[u8]) {
    let compressed = compress_to_vec_zlib(bytes, 6);
    word(out, compressed.len() as u32);
    out.extend_from_slice(&compressed);
}
fn table<T: Asset>(out: &mut Vec<u8>, asset: &T) {
    word(out, 800);
    word(out, 1);
    let mut bytes = 1u32.to_le_bytes().to_vec();
    asset.serialize_exe(&mut bytes, GameVersion::GameMaker8_0).unwrap();
    chunk(out, &bytes);
}
fn empty_table(out: &mut Vec<u8>) {
    word(out, 800);
    word(out, 0);
}
fn code(source: &str) -> CodeAction {
    CodeAction {
        id: 0,
        applies_to: -1,
        is_condition: false,
        invert_condition: false,
        is_relative: false,
        lib_id: 1,
        action_kind: 0,
        execution_type: 2,
        can_be_relative: 0,
        applies_to_something: true,
        fn_name: "".into(),
        fn_code: source.into(),
        param_count: 0,
        param_types: [0; 8],
        param_strings: Default::default(),
    }
}

/// A quiet 440 Hz square wave with a short fade, generated with integer math.
fn wav() -> Vec<u8> {
    const RATE: u32 = 22_050;
    const COUNT: u32 = RATE / 4;
    let mut out = b"RIFF".to_vec();
    word(&mut out, 36 + COUNT * 2);
    out.extend_from_slice(b"WAVEfmt ");
    word(&mut out, 16);
    out.extend_from_slice(&[1, 0, 1, 0]);
    word(&mut out, RATE);
    word(&mut out, RATE * 2);
    out.extend_from_slice(&[2, 0, 16, 0]);
    out.extend_from_slice(b"data");
    word(&mut out, COUNT * 2);
    for i in 0..COUNT {
        let fade = i.min(COUNT - 1 - i).min(220) as i32;
        let amplitude = 3000 * fade / 220;
        let sample = if (i * 440 * 2 / RATE) & 1 == 0 { amplitude } else { -amplitude };
        out.extend_from_slice(&(sample as i16).to_le_bytes());
    }
    out
}

/// Build a complete standard GM8.0 resource container accepted by `from_exe`.
/// The PE/fingerprint bytes are format metadata only, not a Windows runner.
pub fn executable() -> Vec<u8> {
    let sound = asset::Sound {
        name: "snd_fixture".into(),
        source: "fixture.wav".into(),
        extension: ".wav".into(),
        data: Some(wav().into_boxed_slice()),
        kind: asset::SoundKind::Normal,
        volume: 0.5,
        pan: 0.0,
        preload: true,
        fx: asset::sound::SoundFX { chorus: false, echo: false, flanger: false, gargle: false, reverb: false },
    };
    let sprite = asset::Sprite {
        name: "spr_fixture".into(),
        origin_x: 8,
        origin_y: 8,
        frames: vec![asset::sprite::Frame {
            width: 16,
            height: 16,
            // GM8's serialized sprite pixels use BGRA; GL atlas upload converts them.
            data: [170u8, 210, 40, 255].repeat(256).into_boxed_slice(),
        }],
        colliders: vec![asset::sprite::CollisionMap {
            width: 16,
            height: 16,
            bbox_left: 0,
            bbox_right: 15,
            bbox_top: 0,
            bbox_bottom: 15,
            data: vec![true; 256].into_boxed_slice(),
        }],
        per_frame_colliders: false,
    };
    let script = asset::Script {
        name: "fixture_clamp".into(),
        source: "return max(argument1, min(argument2, argument0));".into(),
    };
    let mut events: Vec<Vec<(u32, Vec<CodeAction>)>> = (0..12).map(|_| vec![]).collect();
    events[0].push((0, vec![code(CREATE)]));
    events[3].push((0, vec![code(STEP)]));
    events[8].push((0, vec![code(DRAW)]));
    let object = asset::Object {
        name: "obj_fixture".into(),
        sprite_index: 0,
        solid: false,
        visible: true,
        depth: 0,
        persistent: false,
        parent_index: -1,
        mask_index: -1,
        events,
    };
    let room = asset::Room {
        name: "rm_fixture".into(),
        caption: "PokeNX free fixture".into(),
        width: 1280,
        height: 720,
        speed: 60,
        persistent: false,
        bg_colour: Colour::new(18, 24, 38, 255),
        clear_screen: true,
        clear_region: true,
        creation_code: "global.fixture_started = 1;".into(),
        backgrounds: vec![],
        views_enabled: false,
        views: vec![],
        instances: vec![asset::room::Instance {
            x: 640,
            y: 360,
            object: 0,
            id: 100001,
            creation_code: "".into(),
            xscale: 1.0,
            yscale: 1.0,
            blend: 0xFFFFFF,
            angle: 0.0,
        }],
        tiles: vec![],
        uses_810_features: false,
        uses_811_features: false,
    };

    let mut payload = Vec::new();
    // Garbage count, Pro flag, game id and GUID.
    for value in [0, 1, 0x504E_5854, 1, 2, 3, 4] {
        word(&mut payload, value);
    }
    word(&mut payload, 700);
    word(&mut payload, 0); // extensions
    empty_table(&mut payload); // triggers
    empty_table(&mut payload); // constants
    table(&mut payload, &sound);
    table(&mut payload, &sprite);
    empty_table(&mut payload); // backgrounds
    empty_table(&mut payload); // paths
    table(&mut payload, &script);
    empty_table(&mut payload); // fonts: use runtime's default font
    empty_table(&mut payload); // timelines
    table(&mut payload, &object);
    table(&mut payload, &room);
    word(&mut payload, 100001);
    word(&mut payload, 10000000);
    empty_table(&mut payload); // included files
    word(&mut payload, 800); // help
    let mut help = Vec::new();
    word(&mut help, 0);
    word(&mut help, 0);
    string(&mut help, "Fixture");
    for n in [0, 0, 640, 480, 1, 0, 0, 0] {
        word(&mut help, n);
    }
    string(&mut help, "Original synthetic runtime fixture, GPL-2.0-only.");
    chunk(&mut payload, &help);
    word(&mut payload, 500);
    word(&mut payload, 1);
    string(&mut payload, "global.fixture_library = 1;");
    word(&mut payload, 700);
    word(&mut payload, 1);
    word(&mut payload, 0);

    // Invert GM8's two passes: forward swaps, then a forward substitution delta.
    for i in 0..payload.len() {
        payload.swap(i, i.saturating_sub(i & 255));
    }
    for i in 1..payload.len() {
        payload[i] = payload[i].wrapping_add(payload[i - 1]).wrapping_add(i as u8);
    }

    let header = 0x144B00usize;
    let mut exe = vec![0; header];
    exe[..2].copy_from_slice(b"MZ");
    exe[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
    exe[0x80..0x88].copy_from_slice(b"PE\0\0\x4C\x01\x01\0");
    exe[0x98..0xA0].copy_from_slice(b".data\0\0\0");
    exe[0xA49BE..0xA49C6].copy_from_slice(&[0x8B, 0x45, 0xF4, 0xE8, 0x2A, 0xBD, 0xFD, 0xFF]);
    exe[0xA49C6] = 0x3D;
    exe[0xA49C7..0xA49CB].copy_from_slice(&0x12345678u32.to_le_bytes());
    exe[0xA49CB..0xA49D1].copy_from_slice(&[0x0F, 0x85, 0x18, 0x01, 0, 0]);
    exe[0xA49E2..0xA49E9].copy_from_slice(&[0x8B, 0xC6, 0xE8, 0x07, 0xBD, 0xFD, 0xFF]);
    exe[0xA49E9] = 0x3D;
    exe[0xA49EA..0xA49EE].copy_from_slice(&800u32.to_le_bytes());
    exe[0xA49EE..0xA49F4].copy_from_slice(&[0x0F, 0x85, 0xF5, 0, 0, 0]);
    exe[0x144AC0..0x144AC4].copy_from_slice(&(header as u32).to_le_bytes());
    word(&mut exe, 0x12345678);
    word(&mut exe, 800);
    word(&mut exe, 0);
    word(&mut exe, 0);
    let mut settings = vec![0u32; 34];
    settings[4] = u32::MAX;
    settings[13] = 1;
    settings[22] = 1;
    settings[26] = 255;
    settings[28] = 1;
    settings[29] = 1;
    let settings: Vec<u8> = settings.iter().flat_map(|v| v.to_le_bytes()).collect();
    chunk(&mut exe, &settings);
    word(&mut exe, 0);
    word(&mut exe, 0); // no DLL name or DLL data
    word(&mut exe, 0);
    word(&mut exe, 0); // no decrypt-header garbage
    exe.extend(0..=255u8); // identity substitution permutation
    word(&mut exe, payload.len() as u32);
    exe.extend_from_slice(&payload);
    exe
}
