//! Tests for behaviour this title depends on (ported from the Android adaptation): `.gmspr`
//! sprite files, WAV files that are really MP3 streams, MIDI parsing, tolerant sprite masks.

use crate::game::audio::wav_is_mp3;

#[path = "../../gm8exe/tests/support/fixture.rs"]
pub(crate) mod fixture;

#[test]
fn complete_free_fixture_is_detected_parsed_and_compiled_by_the_real_runtime() {
    let mut bytes = fixture::executable();
    assert!(gm8_format_probe::detect_standard_gm80_executable(bytes.as_mut_slice()).unwrap().is_some());
    let assets = gm8exe::reader::from_exe(&mut bytes, None::<fn(&str)>, true, false).unwrap();
    let mut compiler = crate::gml::Compiler::new();
    for (index, script) in assets.scripts.iter().enumerate() {
        if let Some(script) = script { compiler.register_script(script.name.0.clone(), index); }
    }
    compiler.register_constant("spr_fixture".as_bytes().into(), 0.0);
    compiler.register_constant("snd_fixture".as_bytes().into(), 0.0);
    for script in assets.scripts.iter().flatten() {
        assert!(!compiler.compile(&script.source.0).unwrap().is_empty());
    }
    for object in assets.objects.iter().flatten() {
        for event in object.events.iter().flatten() {
            crate::action::Tree::from_list(&event.1, &mut compiler).unwrap();
        }
    }
    for room in assets.rooms.iter().flatten() {
        compiler.compile(&room.creation_code.0).unwrap();
        for instance in &room.instances { compiler.compile(&instance.creation_code.0).unwrap(); }
    }
    for source in &assets.library_init_strings { compiler.compile(&source.0).unwrap(); }
}

#[cfg(all(windows, feature = "std"))]
#[path = "host_gl_fixture.rs"]
mod host_gl_fixture;

#[test]
fn short_taps_between_simulation_ticks_keep_both_edges_until_the_tick_consumes_them() {
    let mut input = crate::input::Input::new();
    input.button_press(b'Z', true); // a presentation with no simulation tick yet
    input.apply_pending_input();
    input.button_release(b'Z', true); // released before the next slower tick
    input.apply_pending_input();
    assert!(!input.keyboard_check(b'Z'));
    assert!(input.keyboard_check_pressed(b'Z'));
    assert!(input.keyboard_check_released(b'Z'));
    input.step();
    assert!(!input.keyboard_check_pressed(b'Z'));
    assert!(!input.keyboard_check_released(b'Z'));
}

#[test]
fn mirrored_sprite_parts_sample_inside_their_atlas_cell() {
    let uvs = crate::render::sprite_part_uvs(16.0, 8.0, -32.0, -16.0, 64.0, 32.0);
    assert_eq!(uvs, (0.75, 0.75, 0.25, 0.25));
    assert_eq!(crate::render::sprite_part_uvs(16.0, 8.0, 32.0, 16.0, 64.0, 32.0), (0.25, 0.25, 0.75, 0.75));
}

#[test]
fn placeholder_collision_mask_uses_bbox_and_precise_mask_keeps_its_pixels() {
    use crate::{asset::sprite::Collider, math::Real};
    let collider = |data: Vec<bool>| Collider {
        width: 2, height: 2,
        bbox_left: 0, bbox_right: 1, bbox_top: 0, bbox_bottom: 1,
        data: data.into_boxed_slice(),
    }.normalize_mask();
    let hit = |mask: &Collider, x, y| mask.check_collision_point_precise(
        x, y, 0, 0, 0, 0, Real::from(1), Real::from(1), 0.0, 1.0,
    );
    for data in [vec![], vec![false], vec![false; 4]] {
        let mask = collider(data);
        assert!(mask.data.is_empty());
        assert!(hit(&mask, 1, 1));
        assert!(!hit(&mask, 2, 1));
    }
    let precise = collider(vec![true, false, false, false]);
    assert!(hit(&precise, 0, 0));
    assert!(!hit(&precise, 1, 1));
}

#[test]
fn wav_with_mp3_format_tag_is_detected() {
    let mut header = fixture::mp3_wave_with_metadata();
    assert!(wav_is_mp3(&header));
    let fmt = header.windows(4).position(|w| w == b"fmt ").unwrap() + 8;
    header[fmt] = 0x01;
    assert!(!wav_is_mp3(&header), "PCM format tag is a normal WAV");
    assert!(!wav_is_mp3(b"RIFF"), "too short");
}

#[test]
fn wrapped_mp3_metadata_and_malformed_chunks() {
    let wave = fixture::mp3_wave_with_metadata();
    assert!(wav_is_mp3(&wave), "fmt must be located after padded metadata");
    for end in 0..wave.len() {
        assert!(!wav_is_mp3(&wave[..end]), "truncated RIFF at {end}");
    }
    let mut huge_chunk = wave.clone();
    huge_chunk[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(!wav_is_mp3(&huge_chunk));
    let mut short_fmt = wave.clone();
    let fmt = short_fmt.windows(4).position(|w| w == b"fmt ").unwrap();
    short_fmt[fmt + 4..fmt + 8].copy_from_slice(&1u32.to_le_bytes());
    assert!(!wav_is_mp3(&short_fmt));
}

#[test]
fn gmspr_header_is_validated() {
    use crate::gml::kernel::parse_gmspr;
    assert!(parse_gmspr(&[1, 2, 3]).is_err());
    let mut bad_magic = vec![0u8; 16];
    bad_magic[0] = 1;
    assert!(parse_gmspr(&bad_magic).err().unwrap().contains("magic"));
    // Right magic, but the zlib body is garbage.
    let mut bad_body = 0x0012_D591u32.to_le_bytes().to_vec();
    bad_body.extend_from_slice(&[0, 0, 0, 0, 9, 9, 9, 9]);
    assert!(parse_gmspr(&bad_body).is_err());
}

fn tiny_midi() -> Vec<u8> {
    let track: Vec<u8> = vec![
        0x00, 0xFF, 0x51, 0x03, 0x07, 0xA1, 0x20, // tempo 500000 us/beat
        0x00, 0x90, 0x3C, 0x40, // note on middle C
        0x83, 0x60, 0x80, 0x3C, 0x00, // 480 ticks later: note off
        0x00, 0xFF, 0x2F, 0x00, // end of track
    ];
    let mut out = b"MThd".to_vec();
    out.extend_from_slice(&6u32.to_be_bytes());
    out.extend_from_slice(&[0, 0, 0, 1, 0x01, 0xE0]); // format 0, one track, 480 ticks per beat
    out.extend_from_slice(b"MTrk");
    out.extend_from_slice(&(track.len() as u32).to_be_bytes());
    out.extend_from_slice(&track);
    out
}

#[test]
fn midi_files_parse_and_report_a_length() {
    use crate::game::audio::midi::MidiHandle;
    let handle = MidiHandle::new(tiny_midi().into_boxed_slice(), 7, 1.0, true).expect("valid MIDI parses");
    // 0.5 s of stereo at 44.1 kHz.
    assert_eq!(handle.length(), 44_100);
    assert_eq!((handle.sample_rate(), handle.channel_count()), (44_100, 2));
}

#[test]
fn riff_wrapped_midi_is_unwrapped() {
    use crate::game::audio::midi::MidiHandle;
    let midi = tiny_midi();
    let mut wrapped = b"RIFF".to_vec();
    wrapped.extend_from_slice(&(midi.len() as u32 + 12).to_le_bytes());
    wrapped.extend_from_slice(b"RMIDdata");
    wrapped.extend_from_slice(&(midi.len() as u32).to_le_bytes());
    wrapped.extend_from_slice(&midi);
    assert!(MidiHandle::new(wrapped.into_boxed_slice(), 1, 1.0, true).is_some());
    assert!(MidiHandle::new(b"not midi".to_vec().into_boxed_slice(), 1, 1.0, true).is_none());
}

#[test]
fn midi_without_timed_events_does_not_panic() {
    use crate::game::audio::midi::MidiHandle;
    let mut midi = b"MThd".to_vec();
    midi.extend_from_slice(&6u32.to_be_bytes());
    midi.extend_from_slice(&[0, 0, 0, 1, 0x01, 0xE0]);
    midi.extend_from_slice(b"MTrk");
    let track = [0x00u8, 0xFF, 0x2F, 0x00];
    midi.extend_from_slice(&(track.len() as u32).to_be_bytes());
    midi.extend_from_slice(&track);
    // Upstream panics computing the length of a file with no events; the vendored copy returns 0.
    if let Some(handle) = MidiHandle::new(midi.into_boxed_slice(), 1, 1.0, true) {
        assert_eq!(handle.length(), 0);
    }
}
