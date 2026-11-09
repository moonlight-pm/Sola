//! Writes the project page and the manual as plain files into `dist/`, for
//! Cloudflare to serve. Every page comes from files compiled in (the manual
//! among them), so a build is the manual of the commit it was built from.

mod manual;
mod site;

use std::path::Path;

use site::Site;

fn main() {
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("dist");
    if let Err(e) = write(&out) {
        eprintln!("sola-site: could not write {}: {e}", out.display());
        std::process::exit(1);
    }
    println!("sola-site: wrote {}", out.display());
}

fn write(out: &Path) -> std::io::Result<()> {
    if out.exists() {
        std::fs::remove_dir_all(out)?;
    }
    for (path, body) in Site::build().files() {
        let file = out.join(path);
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(file, body)?;
    }
    Ok(())
}
