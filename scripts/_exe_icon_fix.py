from pathlib import Path

cargo_path = Path("Cargo.toml")
cargo = cargo_path.read_text(encoding="utf-8")
old_build_deps = '''[build-dependencies]\nslint-build = "1.17.1"\n'''
new_build_deps = '''[build-dependencies]\nico = "0.5"\nresvg = "0.47"\nslint-build = "1.17.1"\ntiny-skia = { version = "0.12", features = ["png-format"] }\nwinresource = "0.1"\n'''
if cargo.count(old_build_deps) != 1:
    raise SystemExit("Cargo.toml build-dependencies marker changed")
cargo_path.write_text(cargo.replace(old_build_deps, new_build_deps, 1), encoding="utf-8")

build_rs = '''use std::{env, error::Error, fs, io, io::Cursor, path::PathBuf};

const APP_ICON_SIZES: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];
const APP_ICON_SOURCE: &str = "assets/numflow-icon.svg";

fn main() {
    for path in [
        "ui/main.slint",
        "ui/app.slint",
        "ui/design-system.slint",
        "ui/tray.slint",
        "ui/hud.slint",
        APP_ICON_SOURCE,
        "assets/icons/numflow/tray-mouse-left.svg",
        "assets/icons/numflow/tray-mouse-middle.svg",
        "assets/icons/numflow/tray-mouse-right.svg",
        "assets/icons/numflow/tray-mouse-left-held.svg",
        "assets/icons/numflow/tray-mouse-middle-held.svg",
        "assets/icons/numflow/tray-mouse-right-held.svg",
        "assets/icons/numflow/tray-mouse-off.svg",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }

    slint_build::compile("ui/main.slint").expect("failed to compile Slint UI");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_windows_executable_icon().expect("failed to embed NumFlow executable icon");
    }
}

fn embed_windows_executable_icon() -> Result<(), Box<dyn Error>> {
    let svg = fs::read(APP_ICON_SOURCE)?;
    let options = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_data(&svg, &options)?;
    let source_size = tree.size();
    let mut icon_dir = ico::IconDir::new(ico::ResourceType::Icon);

    for size in APP_ICON_SIZES {
        let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size)
            .ok_or_else(|| io::Error::other(format!("invalid icon raster size {size}")))?;
        let size_f32 = f32::from(u16::try_from(size).expect("icon size fits in u16"));
        let transform = resvg::tiny_skia::Transform::from_scale(
            size_f32 / source_size.width(),
            size_f32 / source_size.height(),
        );
        resvg::render(&tree, transform, &mut pixmap.as_mut());

        // PNG encoding demultiplies tiny-skia's internal premultiplied RGBA pixels, preserving
        // antialiased edges and the logo's translucent glow correctly inside the ICO resource.
        let png = pixmap.encode_png()?;
        let image = ico::IconImage::read_png(Cursor::new(png))?;
        icon_dir.add_entry(ico::IconDirEntry::encode_as_png(&image)?);
    }

    let out_dir = PathBuf::from(
        env::var_os("OUT_DIR").ok_or_else(|| io::Error::other("Cargo did not provide OUT_DIR"))?,
    );
    let icon_path = out_dir.join("numflow.ico");
    icon_dir.write(fs::File::create(&icon_path)?)?;

    let icon_path = icon_path
        .to_str()
        .ok_or_else(|| io::Error::other("generated icon path is not valid UTF-8"))?;
    let mut resources = winresource::WindowsResource::new();
    resources.set_icon(icon_path);
    resources.compile()?;

    Ok(())
}
'''
Path("build.rs").write_text(build_rs, encoding="utf-8")
