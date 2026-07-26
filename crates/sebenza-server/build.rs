//! Ensure the embedded-frontend folder (`frontend/dist`) exists at compile time.
//! `rust-embed` requires the folder to exist, but `cargo build`/`cargo test`
//! must work even when the frontend hasn't been built yet — so when it's
//! missing we drop a minimal placeholder `index.html`. A real `npm run build`
//! overwrites it, and a release build then embeds the real bundle.

use std::fs;
use std::path::Path;

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let dist = Path::new(&manifest).join("../../frontend/dist");
    let index = dist.join("index.html");
    if !index.exists() {
        let _ = fs::create_dir_all(&dist);
        let _ = fs::write(
            &index,
            "<!doctype html><meta charset=\"utf-8\"><title>Sebenza</title>\
             <body style=\"font-family:sans-serif;padding:2rem\">\
             Frontend not built. Run <code>cd frontend &amp;&amp; npm run build</code>, \
             then rebuild the server.</body>",
        );
    }
    println!("cargo:rerun-if-changed={}", dist.display());
    println!("cargo:rerun-if-changed=build.rs");
}
