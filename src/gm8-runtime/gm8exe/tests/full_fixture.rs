mod support {
    pub mod fixture;
}

#[test]
fn full_container_parses_resources_events_and_initial_room() {
    let assets = gm8exe::reader::from_exe(support::fixture::executable(), None::<fn(&str)>, true, false)
        .expect("synthetic standard GM8 container must parse completely");
    assert_eq!(assets.game_id, 0x504E_5854);
    assert_eq!(assets.room_order, [0]);
    let room = assets.rooms[0].as_ref().unwrap();
    assert_eq!((room.width, room.height, room.speed), (1280, 720, 60));
    assert_eq!(room.instances[0].object, 0);
    let object = assets.objects[0].as_ref().unwrap();
    for (event, source) in [(0, support::fixture::CREATE), (3, support::fixture::STEP), (8, support::fixture::DRAW)] {
        assert_eq!(&*object.events[event][0].1[0].fn_code.0, source.as_bytes());
    }
    assert_eq!(assets.sprites[0].as_ref().unwrap().frames[0].data.len(), 16 * 16 * 4);
    assert!(assets.sounds[0].as_ref().unwrap().data.as_ref().unwrap().starts_with(b"RIFF"));
    assert_eq!(assets.scripts[0].as_ref().unwrap().name.to_string(), "fixture_clamp");
    assert_eq!(assets.library_init_strings.len(), 1);
}

#[test]
fn truncated_complete_container_returns_errors_without_panicking() {
    let bytes = support::fixture::executable();
    for end in [0, 0x3C, 0x144B00 - 1, 0x144B00 + 20, bytes.len() - 1] {
        assert!(
            gm8exe::reader::from_exe(bytes[..end].to_vec(), None::<fn(&str)>, true, false).is_err(),
            "truncation at {end} must fail"
        );
    }
}
