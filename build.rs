use std::{env, path::Path, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=Cargo.toml");

    // Ask Cargo for the resolved source path instead of assuming a registry,
    // CARGO_HOME layout, or crates.io cache directory. Cargo has already fetched
    // dependencies before running this script. Do not download anything here.
    let manifest = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join("Cargo.toml");
    let output = Command::new(env::var_os("CARGO").unwrap())
        .args([
            "metadata",
            "--format-version=1",
            "--offline",
            "--locked",
            "--filter-platform",
        ])
        .arg(env::var("TARGET").unwrap())
        .arg("--manifest-path")
        .arg(manifest)
        .output()
        .expect("could not run cargo metadata to locate the wordninja dependency");
    assert!(
        output.status.success(),
        "could not locate the resolved wordninja dependency: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("invalid cargo metadata JSON");
    let package = metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|package| package["name"] == "wordninja" && package["version"] == "0.1.0")
        .expect("wordninja 0.1.0 is missing from the resolved dependencies");
    let preset = Path::new(package["manifest_path"].as_str().unwrap())
        .parent()
        .unwrap()
        .join("wordninja_words.txt");
    assert!(
        preset.is_file(),
        "the wordninja 0.1.0 dependency is missing its bundled vocabulary: {}",
        preset.display()
    );
    println!("cargo:rerun-if-changed={}", preset.display());
    println!("cargo:rustc-env=WORDNINJA_PRESET_PATH={}", preset.display());
}
