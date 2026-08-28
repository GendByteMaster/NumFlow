use std::{env, error::Error, fs, io, io::Cursor, path::PathBuf};

const APP_ICON_SIZES: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];
const APP_ICON_SOURCE: &str = "assets/numflow-icon.svg";

fn main() {
    println!("cargo:rerun-if-env-changed=NUMFLOW_UIACCESS");
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
    resources.set_manifest(&windows_manifest(uiaccess_build_enabled()));
    resources.append_rc_content(
        r#"
LANGUAGE 0x9, 0x1
STRINGTABLE
BEGIN
    100 "NumFlow"
    101 "Keyboard-driven pointer control. A minimal NumFlow runtime is used on protected Windows desktops."
END
"#,
    );
    resources.compile()?;

    Ok(())
}

fn uiaccess_build_enabled() -> bool {
    env::var("NUMFLOW_UIACCESS").is_ok_and(|value| value == "1")
}

fn windows_manifest(uiaccess: bool) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity version="1.0.0.0" processorArchitecture="*" name="GendByteMaster.NumFlow" type="win32" />
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="{uiaccess}" />
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}}" />
    </application>
  </compatibility>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <longPathAware xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">true</longPathAware>
    </windowsSettings>
  </application>
</assembly>"#,
        uiaccess = if uiaccess { "true" } else { "false" }
    )
}

#[cfg(test)]
mod tests {
    use super::windows_manifest;

    #[test]
    fn uiaccess_is_opt_in_for_signed_production_artifacts() {
        assert!(windows_manifest(true).contains("uiAccess=\"true\""));
        assert!(windows_manifest(false).contains("uiAccess=\"false\""));
    }
}
