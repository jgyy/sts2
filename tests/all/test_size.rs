//! No source file grows past the cap: a file that long is several files. And no directory of sources holds more
//! files than the cap: past that, the rest go in a new directory.
//! And the recorded answers in `tests/golden/` stay under 100 MB in all: past that, a recording is cut down or dropped.
//! (the original version capped files at 1000 lines of up to 250 columns and directories at 10 files; Rust's lines are
//! shorter and its module tree wider; the caps are the user's of 2026-11-28: 2,000 lines a file, 20 files a directory.)

use std::fs;
use std::path::{Path, PathBuf};

const MAX_LINES: usize = 2000;
const MAX_FILES: usize = 20; // a directory's files, subdirectories not counted
const MAX_GOLDEN_BYTES: u64 = 100_000_000; // tests/golden/ as a whole, every file in it (user rule, 2026-11-28)
const SKIP: [&str; 5] = [".git", ".build", ".claude", "target", "golden"]; // generated, recorded, or not ours

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn walk(dir: &Path, visit: &mut dyn FnMut(&Path, &[PathBuf])) {
    let mut files = Vec::new();
    let mut dirs = Vec::new();
    for entry in fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            if !SKIP.contains(&entry.file_name().to_string_lossy().as_ref()) {
                dirs.push(path);
            }
        } else {
            files.push(path);
        }
    }
    visit(dir, &files);
    for d in dirs {
        walk(&d, visit);
    }
}

#[test]
fn test_no_source_file_is_longer_than_the_cap() {
    let mut seen = 0;
    let mut too_long = Vec::new();
    for top in ["src", "tests", "scripts"] {
        walk(&root().join(top), &mut |_, files| {
            for p in files {
                if matches!(p.extension().and_then(|e| e.to_str()), Some("rs") | Some("sh")) {
                    seen += 1;
                    let n = fs::read_to_string(p).unwrap().lines().count();
                    if n > MAX_LINES {
                        too_long.push((p.strip_prefix(root()).unwrap().display().to_string(), n));
                    }
                }
            }
        });
    }
    assert!(seen > 0);
    assert!(too_long.is_empty(), "split these (cap {MAX_LINES} lines): {too_long:?}");
}

#[test]
fn test_no_directory_holds_more_files_than_the_cap() {
    let mut crowded = Vec::new();
    walk(&root(), &mut |dir, files| {
        if files.len() > MAX_FILES {
            crowded.push((dir.strip_prefix(root()).unwrap().display().to_string(), files.len()));
        }
    });
    assert!(crowded.is_empty(), "move the rest into a new directory (cap {MAX_FILES} files): {crowded:?}");
}

/// Bytes of every file under `dir`.
fn bytes_under(dir: &Path) -> u64 {
    fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|entry| {
            let path = entry.path();
            if path.is_dir() {
                bytes_under(&path)
            } else {
                entry.metadata().map_or(0, |m| m.len())
            }
        })
        .sum()
}

#[test]
fn test_the_golden_directory_stays_under_its_cap() {
    let golden = root().join("tests").join("golden");
    let total = bytes_under(&golden);
    assert!(total > 0, "tests/golden is empty or missing");
    // (the largest first, to say where to cut)
    let mut parts: Vec<(u64, String)> = fs::read_dir(&golden)
        .unwrap()
        .flatten()
        .map(|e| (if e.path().is_dir() { bytes_under(&e.path()) } else { e.metadata().map_or(0, |m| m.len()) }, e.file_name().to_string_lossy().into_owned()))
        .collect();
    parts.sort_by(|a, b| b.cmp(a));
    parts.truncate(5);
    assert!(total <= MAX_GOLDEN_BYTES, "tests/golden holds {total} bytes (cap {MAX_GOLDEN_BYTES}): cut a recording down; the largest: {parts:?}");
}
