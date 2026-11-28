//! No source file grows past the cap: a file that long is several files. And no directory of sources holds more
//! files than the cap: past that, the rest go in a new directory.
//! (the original version capped files at 1000 lines of up to 250 columns and directories at 10 files; Rust's lines are
//! shorter and its module tree wider, so the caps are what the port came out at, with some room.)

use std::fs;
use std::path::{Path, PathBuf};

const MAX_LINES: usize = 2500;
const MAX_FILES: usize = 25; // a directory's files, subdirectories not counted
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
