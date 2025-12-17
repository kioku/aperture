mod common;

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::common::*;

    #[test]
    fn test_binary_cache() {
        let path1 = &*APERTURE_BIN;
        let path2 = &*APERTURE_BIN;
        assert_eq!(path1, path2, "Binary path should be cached");
    }

    #[test]
    fn test_aperture_cmd_helper() {
        let cmd = aperture_cmd();
        // Extract file stem to handle platform differences (aperture vs aperture.exe)
        let program = cmd.get_program();
        let stem = Path::new(program)
            .file_stem()
            .expect("binary should have a file stem")
            .to_string_lossy();
        assert_eq!(stem, "aperture");
    }
}
