use std::{env, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=numflow-input.manifest");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows")
        || env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc")
    {
        return;
    }

    let manifest = PathBuf::from(
        env::var_os("CARGO_MANIFEST_DIR").expect("Cargo must provide CARGO_MANIFEST_DIR"),
    )
    .join("numflow-input.manifest");

    println!("cargo:rustc-link-arg-bin=numflow-input=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg-bin=numflow-input=/MANIFESTUAC:level='asInvoker' uiAccess='true'");
    println!(
        "cargo:rustc-link-arg-bin=numflow-input=/MANIFESTINPUT:{}",
        manifest.display()
    );
}
