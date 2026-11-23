//! Creates a developer-owned application and makes it the workspace default.

use std::borrow::Cow;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;
use toml::Spanned;

use super::bundle::{Arch, Failure, Interrupt, PackageSystem, log_outcome, package_checks};

const STAGING_FOLDER: &str = ".application.new-app";
const STAGING_PREFIX: &str = ".application.new-app-";
const ICON: &[u8] = include_bytes!("../../templates/new-app/icon.png");
const PRODUCT: &str = include_str!("../../templates/new-app/product.toml");
const MANIFEST: &str = include_str!("../../templates/new-app/Cargo.toml");
const README: &str = include_str!("../../templates/new-app/README.md");

/// The options of `cargo xtask new-app`.
#[derive(clap::Args, Debug, Clone, Default)]
pub struct NewAppArgs {
    #[arg(long)]
    pub name: String,
    #[arg(long)]
    pub identifier: String,
    #[arg(long)]
    pub scheme: Option<String>,
    #[arg(long, value_name = "FILE")]
    pub icon: Option<PathBuf>,
    #[arg(long)]
    pub version: Option<String>,
}

/// What `new_app` made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Made {
    pub folder: PathBuf,
    pub package: String,
    pub placeholder_icon: bool,
}

#[derive(Deserialize)]
struct Manifest {
    workspace: Workspace,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
struct Workspace {
    members: Spanned<Vec<Spanned<String>>>,
    default_members: Option<Spanned<Vec<String>>>,
    #[serde(default)]
    exclude: Vec<String>,
}

/// Creates `app/<package>/` and sets it as the workspace default.
///
/// After validation, removes leftover staging folders in the workspace root.
pub fn new_app(root: &Path, args: &NewAppArgs) -> Result<Made, Failure> {
    let _group =
        tracing::info_span!("new-app", name = %args.name, identifier = %args.identifier).entered();
    let result = make(root, args);
    log_outcome(&result, &Interrupt::default());
    result
}

fn make(root: &Path, args: &NewAppArgs) -> Result<Made, Failure> {
    let manifest_path = root.join("Cargo.toml");
    let package = yard_product::file_name(&args.name);
    let member = format!("app/{package}");
    let folder = root.join(&member);
    let manifest =
        fs::read_to_string(&manifest_path).map_err(|error| filesystem(&manifest_path, error))?;
    let workspace = toml::from_str::<Manifest>(&manifest)
        .map_err(|error| filesystem(&manifest_path, error))?
        .workspace;
    let defaults = workspace
        .default_members
        .as_ref()
        .map_or(&[][..], |defaults| defaults.get_ref().as_slice());
    const SAMPLE_MEMBER: &str = "app/yard";
    let folder_exists = !package.is_empty() && exists(&folder)?;
    if folder_exists && member != SAMPLE_MEMBER {
        let reason = if defaults
            .iter()
            .any(|default| normalized(Path::new(default)) == Path::new(&member))
        {
            format!("`{member}/` already exists")
        } else {
            format!("`{member}/` exists. Delete `{member}/` and run again.")
        };
        return Err(refusal(reason));
    }
    if let Some(default) = defaults
        .iter()
        .find(|default| normalized(Path::new(default)) != Path::new(SAMPLE_MEMBER))
    {
        return Err(refusal(format!(
            "workspace already defaults to `{default}`"
        )));
    }
    let scheme = args
        .scheme
        .clone()
        .unwrap_or_else(|| package.replace('-', ""));
    let version = args.version.as_deref().unwrap_or("0.1.0");
    let product = render(
        PRODUCT,
        &[
            ("name", &toml_string(&args.name)),
            ("version", &toml_string(version)),
            ("identifier", &toml_string(&args.identifier)),
            ("scheme", &toml_string(&scheme)),
        ],
    );
    let icon = match &args.icon {
        Some(path) => fs::read(path)
            .map_err(|error| refusal(format!("--icon {}: {error}", path.display())))?,
        None => ICON.to_vec(),
    };

    check_product(root, &product, &icon, args.icon.as_deref(), &package)?;
    // Preserve product-name diagnostics when the target is the sample's folder.
    if folder_exists {
        return Err(refusal(format!("`{member}/` already exists")));
    }
    let wanted = package.replace('-', "_");
    let excluded = expanded(root, workspace.exclude.iter().map(String::as_str))?;
    let members = workspace
        .members
        .get_ref()
        .iter()
        .map(|member| member.get_ref().as_str());
    for member in expanded(root, members.clone())? {
        if member.starts_with(STAGING_FOLDER)
            || member
                .iter()
                .next()
                .is_some_and(|part| part.to_string_lossy().starts_with(STAGING_PREFIX))
            || excluded.iter().any(|path| member.starts_with(path))
        {
            continue;
        }
        let path = root.join(&member).join("Cargo.toml");
        let text = if member.as_os_str().is_empty() {
            Cow::Borrowed(manifest.as_str())
        } else {
            match fs::read_to_string(&path) {
                Ok(text) => Cow::Owned(text),
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(filesystem(&path, error)),
            }
        };
        let manifest: toml::Table =
            toml::from_str(&text).map_err(|error| filesystem(&path, error))?;
        if let Some(name) = manifest
            .get("package")
            .and_then(|table| table.get("name"))
            .and_then(toml::Value::as_str)
            && name.to_ascii_lowercase().replace('-', "_") == wanted
        {
            return Err(refusal(format!(
                "--name would use the existing workspace package name {name:?}"
            )));
        }
    }
    tracing::debug!("checked");

    let stale = root.join(STAGING_FOLDER);
    match fs::remove_dir_all(&stale) {
        Ok(()) => tracing::debug!(path = %stale.display(), "removed stale staging folder"),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(filesystem(&stale, error)),
    }
    for entry in fs::read_dir(root).map_err(|error| filesystem(root, error))? {
        let entry = entry.map_err(|error| filesystem(root, error))?;
        if !entry
            .file_name()
            .to_string_lossy()
            .starts_with(STAGING_PREFIX)
        {
            continue;
        }
        let path = entry.path();
        if entry
            .file_type()
            .map_err(|error| filesystem(&path, error))?
            .is_dir()
        {
            fs::remove_dir_all(&path).map_err(|error| filesystem(&path, error))?;
            tracing::debug!(path = %path.display(), "removed stale staging folder");
        }
    }
    let staging = tempfile::Builder::new()
        .prefix(STAGING_PREFIX)
        .tempdir_in(root)
        .map_err(|error| filesystem(root, error))?;
    let source = staging.path().join("src");
    fs::create_dir(&source).map_err(|error| filesystem(&source, error))?;
    let extensions = staging.path().join("extensions");
    fs::create_dir(&extensions).map_err(|error| filesystem(&extensions, error))?;
    for (file, contents) in [
        ("Cargo.toml", render(MANIFEST, &[("package", &package)])),
        ("product.toml", product),
        (
            "README.md",
            render(README, &[("name", &args.name), ("package", &package)]),
        ),
        (
            "build.rs",
            include_str!("../../templates/new-app/build.rs").into(),
        ),
        (
            "src/main.rs",
            include_str!("../../templates/new-app/src/main.rs").into(),
        ),
        (
            "src/menus.rs",
            include_str!("../../templates/new-app/src/menus.rs").into(),
        ),
    ] {
        write(&staging.path().join(file), contents)?;
    }
    write(&staging.path().join("icon.png"), icon)?;
    tracing::debug!("written");
    fs::rename(staging.path(), &folder).map_err(|error| filesystem(&folder, error))?;
    tracing::debug!("renamed");

    // Expand the globs now that the final folder exists.
    let covered = expanded(root, members)?
        .iter()
        .any(|path| normalized(path) == Path::new(&member));
    let manifest = update_manifest(manifest, workspace, &member, covered);
    let replacement = root.join("Cargo.toml.new-app");
    if exists(&replacement)? {
        fs::remove_file(&replacement).map_err(|error| filesystem(&replacement, error))?;
    }
    write(&replacement, manifest)?;
    fs::rename(&replacement, &manifest_path).map_err(|error| filesystem(&manifest_path, error))?;
    tracing::debug!("workspace");

    Ok(Made {
        folder,
        package,
        placeholder_icon: args.icon.is_none(),
    })
}

fn check_product(
    root: &Path,
    text: &str,
    icon: &[u8],
    icon_source: Option<&Path>,
    package: &str,
) -> Result<(), Failure> {
    let temp = tempfile::tempdir().map_err(|error| filesystem(&std::env::temp_dir(), error))?;
    let file = temp.path().join("product.toml");
    write(&file, text)?;
    let icon_file = temp.path().join("icon.png");
    write(&icon_file, icon)?;
    let prefix = if package.is_empty() {
        String::new()
    } else {
        format!("app/{package}/")
    };
    let product_path = format!("{prefix}product.toml");
    let default_icon = format!("{prefix}icon.png");
    let product = yard_build_config::read(&file).map_err(|reason| {
        let reason = use_display_path(reason, &file, Path::new(&product_path));
        let detail = reason
            .strip_prefix(format!("product config {product_path}: `").as_str())
            .and_then(|detail| detail.split_once('`'));
        let flag = match detail.map(|(key, _)| key) {
            Some("name") => Some("--name"),
            Some("version") => Some("--version"),
            Some("identifier") => Some("--identifier"),
            Some("url_scheme") => Some("--scheme"),
            Some("icon") => Some("--icon"),
            _ => None,
        };
        let reason = if flag == Some("--icon") {
            use_display_path(
                reason,
                &icon_file,
                icon_source.unwrap_or(Path::new(&default_icon)),
            )
        } else {
            reason
        };
        match flag {
            Some(flag) => refusal(format!("{flag}: {reason}")),
            None => refusal(reason),
        }
    })?;
    check_package_name(package)?;
    yard_build_config::build_script::version_numbers(&product.version)
        .map_err(|reason| refusal(format!("--version: {reason}")))?;
    image::load_from_memory_with_format(icon, image::ImageFormat::Png)
        .map_err(|error| refusal(format!("--icon: could not decode PNG: {error}")))?;

    for system in [
        PackageSystem::Macos,
        PackageSystem::Linux,
        PackageSystem::Windows,
    ] {
        for arch in [Arch::X86_64, Arch::Aarch64] {
            package_checks(Path::new(&product_path), &product, system, arch).map_err(|reason| {
                let issue = reason
                    .split_once(": ")
                    .map_or(reason.as_str(), |(_, issue)| issue);
                let flag = if issue.starts_with("name and version")
                    || issue.starts_with("`name` and `version`")
                {
                    "--name and --version"
                } else if issue.starts_with("`identifier`") {
                    "--identifier"
                } else if issue.starts_with("version ") || issue.starts_with("`version` ") {
                    "--version"
                } else {
                    "--name"
                };
                refusal(format!("{flag}: {reason}"))
            })?;
        }
    }

    let sample_path = root.join("app/yard/product.toml");
    let sample =
        yard_build_config::read(&sample_path).map_err(|reason| filesystem(&sample_path, reason))?;
    for (flag, value, sample) in [
        ("--name", &product.name, &sample.name),
        ("--identifier", &product.identifier, &sample.identifier),
        ("--scheme", &product.url_scheme, &sample.url_scheme),
    ] {
        if value.to_lowercase() == sample.to_lowercase() {
            return Err(refusal(format!(
                "{flag} must differ from the sample application's"
            )));
        }
    }
    if package == yard_product::file_name(&sample.name) {
        return Err(refusal(format!(
            "--name would use the sample application's Linux name {package:?}"
        )));
    }
    Ok(())
}

fn use_display_path(reason: String, staged: &Path, display: &Path) -> String {
    if let Ok(canonical) = staged.canonicalize() {
        let canonical = canonical.to_string_lossy();
        if reason.contains(canonical.as_ref()) {
            return reason.replacen(canonical.as_ref(), &display.to_string_lossy(), 1);
        }
    }
    reason.replacen(
        staged.to_string_lossy().as_ref(),
        &display.to_string_lossy(),
        1,
    )
}

fn check_package_name(package: &str) -> Result<(), Failure> {
    if package.starts_with(|character: char| character.is_ascii_digit()) {
        return Err(refusal(
            "--name must give a Cargo package name that starts with a letter",
        ));
    }
    if matches!(
        package,
        "build"
            | "deps"
            | "examples"
            | "incremental"
            | "as"
            | "async"
            | "await"
            | "break"
            | "const"
            | "continue"
            | "crate"
            | "dyn"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "gen"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
            | "abstract"
            | "become"
            | "box"
            | "do"
            | "final"
            | "macro"
            | "override"
            | "priv"
            | "try"
            | "typeof"
            | "unsized"
            | "virtual"
            | "yield"
    ) {
        return Err(refusal(format!(
            "--name gives a reserved Cargo name {package:?}"
        )));
    }
    Ok(())
}

fn refusal(reason: impl Into<String>) -> Failure {
    Failure {
        step: "arguments",
        reason: reason.into(),
    }
}

fn filesystem(path: &Path, error: impl std::fmt::Display) -> Failure {
    Failure {
        step: "filesystem",
        reason: format!("{}: {error}", path.display()),
    }
}

fn exists(path: &Path) -> Result<bool, Failure> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(filesystem(path, error)),
    }
}

fn write(path: &Path, contents: impl AsRef<[u8]>) -> Result<(), Failure> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .and_then(|mut file| file.write_all(contents.as_ref()))
        .map_err(|error| filesystem(path, error))
}

fn toml_string(value: &str) -> String {
    toml::Value::String(value.into()).to_string()
}

fn render(template: &str, values: &[(&str, &str)]) -> String {
    let mut parts = template.split("{{");
    let mut output = parts.next().unwrap_or_default().to_owned();
    for part in parts {
        let (marker, rest) = part.split_once("}}").expect("closed template marker");
        let (_, value) = values
            .iter()
            .find(|(key, _)| *key == marker)
            .expect("known template marker");
        output.push_str(value);
        output.push_str(rest);
    }
    output
}

fn normalized(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir if normalized.file_name().is_some_and(|name| name != "..") => {
                normalized.pop();
            }
            _ => normalized.push(part),
        }
    }
    normalized
}

fn update_manifest(mut text: String, workspace: Workspace, member: &str, covered: bool) -> String {
    let members = workspace.members.span();
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let member = toml_string(member);
    let default_value = format!("[{member}]");
    let mut edits = Vec::new();
    if let Some(default) = workspace.default_members {
        edits.push((default.span(), default_value));
    } else {
        let (position, line) = match text[members.end..].find('\n') {
            Some(offset) => (
                members.end + offset + 1,
                format!("default-members = {default_value}{newline}"),
            ),
            None => (
                text.len(),
                format!("{newline}default-members = {default_value}"),
            ),
        };
        edits.push((position..position, line));
    }

    if !covered {
        let close = members.end - 1;
        let last = workspace.members.get_ref().last();
        let tail = last.map_or(members.start + 1, |entry| entry.span().end);
        let trailing_comma = text[tail..close]
            .lines()
            .any(|line| line.split('#').next().unwrap_or_default().trim() == ",");
        if let Some(line) = text[members.start..close]
            .rfind('\n')
            .map(|offset| members.start + offset + 1)
            && text[line..close].trim().is_empty()
        {
            let indent = last
                .and_then(|entry| {
                    let before = &text[..entry.span().start];
                    let prefix = before.rsplit('\n').next().unwrap_or_default();
                    prefix.trim().is_empty().then_some(prefix)
                })
                .unwrap_or("    ");
            let comma = if trailing_comma { "," } else { "" };
            edits.push((line..line, format!("{indent}{member}{comma}{newline}")));
            if last.is_some() && !trailing_comma {
                edits.push((tail..tail, ",".into()));
            }
        } else {
            let (point, value) = match (last, trailing_comma) {
                (Some(_), false) => (tail, format!(", {member}")),
                (_, true) => (close, format!(" {member},")),
                (None, false) => (close, member),
            };
            edits.push((point..point, value));
        }
    }
    edits.sort_by_key(|(span, _)| std::cmp::Reverse(span.start));
    for (span, value) in edits {
        text.replace_range(span, &value);
    }
    text
}

fn expanded<'a>(
    root: &Path,
    patterns: impl IntoIterator<Item = &'a str>,
) -> Result<Vec<PathBuf>, Failure> {
    let mut members = Vec::new();
    for pattern in patterns {
        let parts: Vec<_> = pattern
            .split('/')
            .filter(|part| !matches!(*part, "" | "."))
            .collect();
        expand(root, Path::new(""), &parts, &mut members)?;
    }
    Ok(members)
}

fn expand(
    root: &Path,
    dir: &Path,
    parts: &[&str],
    members: &mut Vec<PathBuf>,
) -> Result<(), Failure> {
    let Some((&part, tail)) = parts.split_first() else {
        members.push(dir.to_owned());
        return Ok(());
    };
    if !part.contains(['*', '?', '[']) {
        return expand(root, &dir.join(part), tail, members);
    }
    if part == "**" {
        expand(root, dir, tail, members)?;
    }
    let folder = root.join(dir);
    let entries = match fs::read_dir(&folder) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(filesystem(&folder, error)),
    };
    for entry in entries {
        let entry = entry.map_err(|error| filesystem(&folder, error))?;
        let name = entry.file_name();
        let path = dir.join(&name);
        let kind = entry
            .file_type()
            .map_err(|error| filesystem(&entry.path(), error))?;
        if part == "**" {
            if kind.is_dir() {
                expand(root, &path, parts, members)?;
            }
        } else if (kind.is_dir() || kind.is_symlink()) && wildcard(&name.to_string_lossy(), part) {
            expand(root, &path, tail, members)?;
        }
    }
    Ok(())
}

fn wildcard(name: &str, pattern: &str) -> bool {
    fn matches(name: &str, pattern: &str) -> bool {
        let Some(token) = pattern.chars().next() else {
            return name.is_empty();
        };
        let rest = &pattern[token.len_utf8()..];
        if token == '*' {
            return name
                .char_indices()
                .any(|(index, _)| matches(&name[index..], rest))
                || matches("", rest);
        }
        let Some(character) = name.chars().next() else {
            return false;
        };
        let remaining = &name[character.len_utf8()..];
        match token {
            '?' => matches(remaining, rest),
            '[' => {
                let inverted = rest.starts_with('!');
                let set = rest.strip_prefix('!').unwrap_or(rest);
                let start = usize::from(set.starts_with(']'));
                let Some(end) = set[start..].find(']').map(|end| start + end) else {
                    return false;
                };
                let mut set_chars = set[..end].chars().peekable();
                let mut found = false;
                while let Some(first) = set_chars.next() {
                    if set_chars.next_if_eq(&'-').is_some() {
                        found |= match set_chars.next() {
                            Some(last) => (first..=last).contains(&character),
                            None => character == first || character == '-',
                        };
                    } else {
                        found |= first == character;
                    }
                }
                found != inverted && matches(remaining, &set[end + 1..])
            }
            _ => character == token && matches(remaining, rest),
        }
    }
    matches(name, pattern)
}
