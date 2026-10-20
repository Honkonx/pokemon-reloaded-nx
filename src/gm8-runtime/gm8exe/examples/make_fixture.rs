#[path = "../tests/support/fixture.rs"]
mod fixture;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = std::env::args_os().nth(1).ok_or("provide a NEW output directory (outside the SD package)")?;
    // Refuse to overwrite a real game directory or any existing fixture.
    std::fs::create_dir(&directory)?;
    std::fs::create_dir(std::path::Path::new(&directory).join("data"))?;
    std::fs::write(
        std::path::Path::new(&directory).join("data/fixture.txt"),
        "Original synthetic PokeNX fixture data. GPL-2.0-only. No Pokemon assets.\n",
    )?;
    let path = std::path::Path::new(&directory).join("pokenx-fixture.exe");
    std::fs::write(&path, fixture::executable())?;
    println!("Synthetic data container: {} (not a Windows program)", path.display());
    Ok(())
}
