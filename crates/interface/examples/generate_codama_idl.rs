use codama::{Codama, NodeTrait};
use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output_path = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| crate_dir.join("../../target/idl/roshi.codama.json"));

    let mut idl = Codama::load(crate_dir)?.get_idl()?;
    idl.program.name = "roshi".into();
    let idl = idl.to_json_pretty()?;

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&output_path, idl)?;
    println!("{}", output_path.display());

    Ok(())
}
