//! The build script of an application built on YARD: an application's
//! `build.rs` is `fn main() { yard_build_config::build_script::run() }`.
//!
//! It reads the product config, gives its values to the code as
//! `PRODUCT_*` variables, tracks the config and the icon for rebuilds,
//! writes the window icon for Linux and FreeBSD, and makes the Windows
//! resource, compiled on a Windows host only.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

/// Runs as the build script: reads Cargo's environment, prints the `cargo:`
/// lines of [`directives`], and for a Windows target compiles the resource.
/// Any error panics with a message naming the config.
pub fn run() {
    let path = env::var_os("PRODUCT_CONFIG").map_or_else(
        || PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("product.toml"),
        PathBuf::from,
    );
    // A build script runs on the host, so the target is read from Cargo.
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let lines = directives(&path, &target_os, &out_dir).unwrap_or_else(|error| panic!("{error}"));
    for line in lines {
        println!("{line}");
    }
    if target_os == "windows" {
        let resource = out_dir.join("app.rc");
        let _group =
            tracing::info_span!("windows-resource", resource = %resource.display()).entered();
        compile_windows_resource(&resource, &path);
    }
}

/// Reads the product config at `config`, writes the files that `target_os`
/// needs into `out_dir` (`window_icon.png` for Linux and FreeBSD, `app.ico`
/// and `app.rc` for Windows), and returns the `cargo:` lines of the build
/// script, inside the group `build-script{config, target_os}`. On an error
/// it writes nothing more, and the error names the config.
pub fn directives(config: &Path, target_os: &str, out_dir: &Path) -> Result<Vec<String>, String> {
    let _group =
        tracing::info_span!("build-script", config = %config.display(), target_os).entered();
    let path = config;
    let mut lines = vec![
        "cargo:rerun-if-env-changed=PRODUCT_CONFIG".to_owned(),
        format!("cargo:rerun-if-changed={}", path.display()),
    ];
    let config = crate::read(path)?;
    lines.extend([
        format!("cargo:rerun-if-changed={}", config.icon.display()),
        format!("cargo:rustc-env=PRODUCT_NAME={}", config.name),
        format!("cargo:rustc-env=PRODUCT_VERSION={}", config.version),
        format!("cargo:rustc-env=PRODUCT_IDENTIFIER={}", config.identifier),
        format!("cargo:rustc-env=PRODUCT_URL_SCHEME={}", config.url_scheme),
        format!("cargo:rustc-env=PRODUCT_ICON={}", config.icon.display()),
        format!(
            "cargo:rustc-env=PRODUCT_UPDATE_URL={}",
            config.update_url.unwrap_or_default()
        ),
    ]);
    let in_config = |error: String| format!("product config {}: {error}", path.display());
    if needs_window_icon(target_os) {
        let window_icon = out_dir.join("window_icon.png");
        write_window_icon(&config.icon, &window_icon).map_err(in_config)?;
        tracing::debug!(file = %window_icon.display(), "written");
    }
    if target_os == "windows" {
        let icon = out_dir.join("app.ico");
        write_windows_icon(&config.icon, &icon).map_err(in_config)?;
        tracing::debug!(file = %icon.display(), "written");
        let resource = out_dir.join("app.rc");
        let script = windows_resource(&icon, &config.name, &config.version).map_err(in_config)?;
        fs::write(&resource, script).map_err(|error| {
            in_config(format!("writing {} failed: {error}", resource.display()))
        })?;
        tracing::debug!(file = %resource.display(), "written");
    }
    Ok(lines)
}

/// The sizes of the `.exe`'s icon, as Windows uses them: the title bar and
/// small icons, the taskbar, Explorer's views, and the largest tiles.
pub const WINDOWS_ICON_SIZES: [u32; 6] = [16, 24, 32, 48, 64, 256];

/// Writes the config's icon as an ICO to `out`, with one PNG frame for each
/// of [`WINDOWS_ICON_SIZES`], resized with Lanczos3 as the other icons are.
pub fn write_windows_icon(icon: &Path, out: &Path) -> Result<(), String> {
    let bytes = fs::read(icon)
        .map_err(|error| format!("reading the icon {} failed: {error}", icon.display()))?;
    let image = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
        .map_err(|error| format!("decoding the icon {} failed: {error}", icon.display()))?;
    let frames = WINDOWS_ICON_SIZES
        .iter()
        .map(|&size| {
            let resized = image
                .resize_exact(size, size, image::imageops::FilterType::Lanczos3)
                .into_rgba8();
            image::codecs::ico::IcoFrame::as_png(
                resized.as_raw(),
                size,
                size,
                image::ExtendedColorType::Rgba8,
            )
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("encoding the icon {} failed: {error}", icon.display()))?;
    let file = fs::File::create(out)
        .map_err(|error| format!("writing {} failed: {error}", out.display()))?;
    image::codecs::ico::IcoEncoder::new(std::io::BufWriter::new(file))
        .encode_images(&frames)
        .map_err(|error| format!("writing {} failed: {error}", out.display()))
}

/// The resource script of the `.exe`: the icon as resource 1, which GPUI
/// loads for the window and the taskbar, and the version information that
/// Explorer shows, from the config's name and version. An error when the
/// version does not fit Windows' version numbers.
pub fn windows_resource(icon: &Path, name: &str, version: &str) -> Result<String, String> {
    let numbers = version_numbers(version)?;
    let icon = rc_string(&icon.display().to_string());
    let name = rc_string(name);
    let version = rc_string(version);
    // The statements and blocks follow Microsoft's VERSIONINFO resource:
    // https://learn.microsoft.com/en-us/windows/win32/menurc/versioninfo-resource
    // The string block 040904B0 is U.S. English (0x409) in Unicode (1200),
    // the pair that Translation names. rc.exe reads the script in the ANSI
    // code page unless told UTF-8, and a name or a path may hold any
    // character.
    Ok(format!(
        r#"#pragma code_page(65001)
1 ICON "{icon}"

1 VERSIONINFO
PRODUCTVERSION {numbers}
FILEVERSION {numbers}
{{
    BLOCK "VarFileInfo"
    {{
        VALUE "Translation", 0x409, 1200
    }}
    BLOCK "StringFileInfo"
    {{
        BLOCK "040904B0"
        {{
            VALUE "ProductName", "{name}"
            VALUE "ProductVersion", "{version}"
            VALUE "FileDescription", "{name}"
            VALUE "FileVersion", "{version}"
        }}
    }}
}}
"#
    ))
}

/// The four numbers of FILEVERSION and PRODUCTVERSION: the version's core
/// `major.minor.patch`, cut at the first `-` or `+`, then 0. A part that is
/// missing or not a number is 0; a number over 65535 is an error, as Windows
/// keeps each in 16 bits.
pub fn version_numbers(version: &str) -> Result<String, String> {
    let core = version.split(['-', '+']).next().unwrap_or("");
    let mut numbers = Vec::new();
    for part in core.split('.').chain(std::iter::repeat("")).take(3) {
        let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
        let number = if digits.is_empty() {
            0
        } else {
            digits
                .parse::<u16>()
                .map_err(|_| {
                    format!(
                        "`version` {version:?}: Windows limits each number of a version to 65535, and {digits} is larger"
                    )
                })?
        };
        numbers.push(number.to_string());
    }
    numbers.push("0".to_owned());
    Ok(numbers.join(","))
}

/// A string of a resource script: a backslash is written twice, and a
/// double quote as two double quotes.
fn rc_string(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\"\"")
}

/// Compiles the resource script and links it into the `.exe`. It needs
/// Windows' resource compiler, so it runs only on a Windows host; a build
/// for Windows on another host has no icon or version in its `.exe`. A
/// build dependency is compiled for the host, so `cfg(windows)` is the
/// host's.
#[cfg(windows)]
fn compile_windows_resource(resource: &Path, config: &Path) {
    if let Err(error) = embed_resource::compile(resource, embed_resource::NONE).manifest_required()
    {
        tracing::error!(%error, "compiling the Windows resource failed");
        panic!(
            "product config {}: compiling the Windows resource {} failed: {error}",
            config.display(),
            resource.display()
        );
    }
}

#[cfg(not(windows))]
fn compile_windows_resource(resource: &Path, _config: &Path) {
    let warning = format!(
        "the Windows resource {} is compiled only on Windows: this .exe has no icon or version",
        resource.display()
    );
    println!("cargo:warning={warning}");
    tracing::warn!("{warning}");
}

/// Whether a build for `target_os` embeds the window icon, as X11 needs.
pub fn needs_window_icon(target_os: &str) -> bool {
    matches!(target_os, "linux" | "freebsd")
}

/// Writes the config's icon as a 256 × 256 PNG to `out`, which the
/// application embeds as the window icon on X11. The icon is decoded as a
/// PNG whatever its file name, as [`crate::read`] checked it by its bytes.
pub fn write_window_icon(icon: &Path, out: &Path) -> Result<(), String> {
    let bytes = fs::read(icon)
        .map_err(|error| format!("reading the icon {} failed: {error}", icon.display()))?;
    image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
        .map_err(|error| format!("decoding the icon {} failed: {error}", icon.display()))?
        .resize_exact(256, 256, image::imageops::FilterType::CatmullRom)
        .save_with_format(out, image::ImageFormat::Png)
        .map_err(|error| format!("writing {} failed: {error}", out.display()))
}
