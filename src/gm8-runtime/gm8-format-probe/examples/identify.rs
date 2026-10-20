use std::{env, fs, process};

fn main() {
    let Some(path) = env::args_os().nth(1) else {
        eprintln!("usage: cargo run --example identify -- <game.exe>");
        process::exit(2);
    };
    let mut bytes = fs::read(path).unwrap_or_else(|error| {
        eprintln!("could not read executable: {error}");
        process::exit(2);
    });
    let header = match gm8_format_probe::detect_standard_gm80(bytes.as_mut_slice()) {
        Ok(Some(header)) => header,
        Ok(None) => {
            println!("not recognized by the standard GM8.0 detector");
            return;
        },
        Err(error) => {
            eprintln!("probe read failed: {error:?}");
            process::exit(1);
        },
    };
    let decrypt_offset =
        match gm8_format_probe::locate_standard_gm80_decrypt_payload(&bytes, header.payload_offset as usize) {
            Ok(offset) => offset,
            Err(error) => {
                eprintln!("could not locate GM8.0 decryption payload: {error:?}");
                process::exit(1);
            },
        };
    match gm8_format_probe::decrypt_standard_gm80_payload(&mut bytes, decrypt_offset) {
        Ok(payload) => println!(
            "standard GM8.0 payload decrypted: parser offset 0x{:X}, {} bytes, Pro={}, game_id=0x{:08X}",
            payload.data_offset, payload.encrypted_data_len, payload.pro_flag, payload.game_id
        ),
        Err(error) => {
            eprintln!("GM8.0 payload validation failed: {error:?}");
            process::exit(1);
        },
    }
}
