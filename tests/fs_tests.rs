use aperture_cli::fs::{FileSystem, OsFileSystem};
use std::fs;
use std::path::PathBuf;

// Helper function to create a temporary directory for tests
fn create_temp_dir(test_name: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push("aperture_test");
    path.push(test_name);
    if path.exists() {
        fs::remove_dir_all(&path).unwrap();
    }
    fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn test_read_to_string() {
    let temp_dir = create_temp_dir("test_read_to_string");
    let file_path = temp_dir.join("test.txt");
    fs::write(&file_path, "Hello, world!").unwrap();

    let fs = OsFileSystem;
    let content = fs.read_to_string(&file_path).unwrap();
    assert_eq!(content, "Hello, world!");
}

#[test]
fn test_write_all() {
    let temp_dir = create_temp_dir("test_write_all");
    let file_path = temp_dir.join("test.txt");

    let fs = OsFileSystem;
    fs.write_all(&file_path, b"Hello, Rust!").unwrap();

    let content = fs::read_to_string(&file_path).unwrap();
    assert_eq!(content, "Hello, Rust!");
}

#[test]
fn test_create_dir_all() {
    let temp_dir = create_temp_dir("test_create_dir_all");
    let new_dir_path = temp_dir.join("new_dir/subdir");

    let fs = OsFileSystem;
    fs.create_dir_all(&new_dir_path).unwrap();

    assert!(new_dir_path.is_dir());
}

#[test]
fn test_remove_file() {
    let temp_dir = create_temp_dir("test_remove_file");
    let file_path = temp_dir.join("test.txt");
    fs::write(&file_path, "delete me").unwrap();

    let fs = OsFileSystem;
    fs.remove_file(&file_path).unwrap();

    assert!(!file_path.exists());
}

#[test]
fn test_remove_dir_all() {
    let temp_dir = create_temp_dir("test_remove_dir_all");
    let dir_to_remove = temp_dir.join("dir_to_remove/subdir");
    fs::create_dir_all(&dir_to_remove).unwrap();
    fs::write(dir_to_remove.join("file.txt"), "content").unwrap();

    let fs = OsFileSystem;
    fs.remove_dir_all(&temp_dir).unwrap();

    assert!(!temp_dir.exists());
}

#[test]
fn test_exists() {
    let temp_dir = create_temp_dir("test_exists");
    let file_path = temp_dir.join("test.txt");
    fs::write(&file_path, "").unwrap();

    let fs = OsFileSystem;
    assert!(fs.exists(&file_path));
    assert!(!fs.exists(&temp_dir.join("non_existent_file.txt")));
}

#[test]
fn test_is_dir() {
    let temp_dir = create_temp_dir("test_is_dir");
    let file_path = temp_dir.join("test.txt");
    fs::write(&file_path, "").unwrap();

    let fs = OsFileSystem;
    assert!(fs.is_dir(&temp_dir));
    assert!(!fs.is_dir(&file_path));
}

#[test]
fn test_is_file() {
    let temp_dir = create_temp_dir("test_is_file");
    let file_path = temp_dir.join("test.txt");
    fs::write(&file_path, "").unwrap();

    let fs = OsFileSystem;
    assert!(fs.is_file(&file_path));
    assert!(!fs.is_file(&temp_dir));
}

#[test]
fn test_canonicalize() {
    let temp_dir = create_temp_dir("test_canonicalize");
    let file_path = temp_dir.join("test.txt");
    fs::write(&file_path, "").unwrap();

    let fs = OsFileSystem;
    let canonical_path = fs.canonicalize(&file_path).unwrap();
    assert_eq!(canonical_path, file_path.canonicalize().unwrap());
}

#[test]
fn test_read_dir() {
    let temp_dir = create_temp_dir("test_read_dir");
    let file1 = temp_dir.join("file1.txt");
    let file2 = temp_dir.join("file2.txt");
    let subdir = temp_dir.join("subdir");
    fs::write(&file1, "").unwrap();
    fs::write(&file2, "").unwrap();
    fs::create_dir(&subdir).unwrap();

    let fs = OsFileSystem;
    let mut entries = fs.read_dir(&temp_dir).unwrap();
    entries.sort(); // Sort for consistent assertion

    let mut expected_entries: Vec<PathBuf> = vec![file1, file2, subdir];
    expected_entries.sort();

    assert_eq!(entries, expected_entries);
}
