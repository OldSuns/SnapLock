use chrono::Local;
use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static CAPTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub fn capture_path(directory: &Path, prefix: &str, extension: &str) -> PathBuf {
    let timestamp = Local::now().format("%Y%m%d_%H%M%S_%9f");
    let sequence = CAPTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    directory.join(format!(
        "{prefix}{timestamp}_{}_{sequence}.{extension}",
        std::process::id()
    ))
}

/// Atomically creates a photo so even an external filename collision cannot overwrite data.
pub fn create_photo(directory: &Path) -> io::Result<(PathBuf, File)> {
    loop {
        let path = capture_path(directory, "snaplock_capture_", "jpg");
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::io::Write;

    #[test]
    fn rapid_captures_keep_distinct_paths_and_existing_photos() {
        let directory = capture_path(Path::new(r"D:\Temp\Pi"), "snaplock-names-", "test");
        std::fs::create_dir(&directory).unwrap();
        let (first_path, mut first_file) = create_photo(&directory).unwrap();
        first_file.write_all(b"original photo").unwrap();
        drop(first_file);
        let mut paths = HashSet::new();
        for _ in 0..100 {
            let (path, file) = create_photo(&directory).unwrap();
            assert!(paths.insert(path));
            drop(file);
        }
        assert_eq!(std::fs::read(first_path).unwrap(), b"original photo");
        let recording_paths: HashSet<_> = (0..100)
            .map(|_| capture_path(&directory, "snaplock_video_", "mkv"))
            .collect();
        assert_eq!(recording_paths.len(), 100);
    }
}
