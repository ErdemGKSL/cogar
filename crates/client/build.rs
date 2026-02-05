use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let html_path = Path::new("web/index.html");
    let src_dir = Path::new("src");
    let safelist_path = Path::new("web/_safelist.html");
    let input_css = Path::new("web/tailwind.input.css");

    println!("cargo:rerun-if-changed={}", html_path.display());
    println!("cargo:rerun-if-changed={}", input_css.display());

    // --- Collect classes from Rust sources ---
    let mut rust_classes: Vec<String> = Vec::new();
    for path in find_rs_files(src_dir) {
        println!("cargo:rerun-if-changed={}", path.display());
        let code = fs::read_to_string(&path).unwrap_or_default();
        rust_classes.extend(extract_rust_classes(&code));
    }

    // --- Write safelist HTML so tailwindcss scans dynamic classes ---
    // Every class token found in Rust source is listed in a single element.
    // web/tailwind.input.css has `@source "./"` so this file is picked up.
    if !rust_classes.is_empty() {
        let class_str = rust_classes.join(" ");
        fs::write(
            safelist_path,
            format!("<div class=\"{}\">\n</div>\n", class_str),
        )
        .expect("failed to write safelist HTML");
    }

    // --- Ensure node_modules is present ---
    if !Path::new("node_modules/.package-lock.json").exists()
        && !Path::new("node_modules/.modules.yaml").exists()
    {
        let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
        let status = Command::new(npm)
            .args(["install", "--ignore-scripts"])
            .status()
            .expect("failed to run npm install (is npm on PATH?)");
        assert!(status.success(), "npm install failed");
    }

    // --- Run tailwindcss CLI ---
    let tw_bin = Path::new("node_modules/.bin/tailwindcss");
    let tw_cmd = if cfg!(windows) {
        "node_modules/.bin/tailwindcss.cmd"
    } else {
        "node_modules/.bin/tailwindcss"
    };
    // Fall back to npx if the local binary doesn't exist
    let status = if tw_bin.exists() || cfg!(windows) {
        Command::new(tw_cmd)
            .args(["-i", "web/tailwind.input.css", "-o", "web/tailwind.css"])
            .status()
            .expect("failed to run tailwindcss")
    } else {
        Command::new("npx")
            .args(["tailwindcss", "-i", "web/tailwind.input.css", "-o", "web/tailwind.css"])
            .status()
            .expect("failed to run tailwindcss via npx")
    };

    // --- Clean up safelist (belt-and-suspenders: always remove) ---
    let _ = fs::remove_file(safelist_path);

    assert!(status.success(), "tailwindcss exited with non-zero status");
}

// ──────────────── Rust source scanner ────────────────

/// Recursively collect every `*.rs` file under `dir`.
fn find_rs_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect_rs(dir, &mut out);
    out
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension() == Some(std::ffi::OsStr::new("rs")) {
            out.push(path);
        }
    }
}

/// Extract class-name strings from Rust source using literal prefix/suffix
/// scanning.  Three patterns that appear in this codebase are handled:
///
/// * `set_class_name("…")`          – direct DOM class assignment
/// * `set_attribute("class", "…")`  – attribute-based assignment
/// * `class=\"…\"`                  – HTML templates inside format! strings
fn extract_rust_classes(code: &str) -> Vec<String> {
    let mut out = Vec::new();
    extract_between(code, "set_class_name(\"", "\"", &mut out);
    extract_between(code, "set_attribute(\"class\", \"", "\"", &mut out);
    // In .rs source the escaped quotes are literal `\"` bytes on disk
    extract_between(code, "class=\\\"", "\\\"", &mut out);
    out
}

/// Generic: find every `prefix … suffix` span and push the middle.
fn extract_between(code: &str, prefix: &str, suffix: &str, out: &mut Vec<String>) {
    let mut rest = code;
    while let Some(idx) = rest.find(prefix) {
        rest = &rest[idx + prefix.len()..];
        let Some(end) = rest.find(suffix) else {
            break;
        };
        let value = &rest[..end];
        if !value.is_empty() {
            out.push(value.to_string());
        }
        rest = &rest[end + suffix.len()..];
    }
}
