mod common;

#[cfg(test)]
mod tests {
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
        // This should not panic and should create a valid command
        assert!(cmd.get_program().to_string_lossy().ends_with("aperture"));
    }
}
