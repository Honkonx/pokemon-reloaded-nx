use std::{env, fs, io::Cursor, process};

fn main() {
    let Some(path) = env::args_os().nth(1) else {
        eprintln!("usage: cargo run -p gm8exe --example identify -- <game.exe>");
        process::exit(2);
    };
    let mut bytes = fs::read(&path).unwrap_or_else(|error| {
        eprintln!("could not read executable: {error}");
        process::exit(2);
    });
    let mut check = Cursor::new(bytes.as_mut_slice());
    match gm8exe::gamedata::gm80::check(&mut check, None::<fn(&str)>) {
        Ok(true) => {
            let payload = check.position() as usize;
            let first = u32::from_le_bytes(check.get_ref()[payload..payload + 4].try_into().unwrap());
            let second = u32::from_le_bytes(check.get_ref()[payload + 4..payload + 8].try_into().unwrap());
            println!("OpenGMK GM8.0 detector leaves payload cursor at 0x{:X} (raw={first},{second})", check.position());
        },
        Ok(false) => println!("OpenGMK standard GM8.0 detector did not match"),
        Err(error) => eprintln!("OpenGMK GM8.0 detector failed: {error}"),
    }
    let logger = |message: &str| {
        if message.contains("UPX")
            || message.contains("antidec")
            || message.contains("GM8.0")
            || message.contains("Decrypting asset data")
            || message.contains("Reading header")
        {
            println!("{message}");
        }
    };
    match gm8exe::reader::from_exe(&mut bytes, Some(logger), false, false) {
        Ok(game) => println!(
            "OpenGMK parsed {:?}: game_id=0x{:08X}, {} sprites, {} sounds, {} backgrounds, {} rooms",
            game.version,
            game.game_id,
            game.sprites.len(),
            game.sounds.len(),
            game.backgrounds.len(),
            game.rooms.len(),
        ),
        Err(error) => {
            eprintln!("OpenGMK parser rejected executable: {error}");
            process::exit(1);
        },
    }
}
