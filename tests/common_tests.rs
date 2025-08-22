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

    #[tokio::test]
    async fn test_mock_server_pool() {
        let server1 = get_mock_server().await;
        let port1 = server1.address().port();
        return_mock_server(server1);

        let server2 = get_mock_server().await;
        let port2 = server2.address().port();

        // Should reuse the same server (same port indicates reuse)
        assert_eq!(port1, port2, "Should reuse mock server from pool");
        return_mock_server(server2);
    }

    #[test]
    fn test_temp_dir_pool() {
        let temp_dir1 = get_temp_dir().unwrap();
        let path1 = temp_dir1.path().to_path_buf();
        return_temp_dir(temp_dir1);

        let temp_dir2 = get_temp_dir().unwrap();
        let path2 = temp_dir2.path().to_path_buf();

        // Should reuse the same temp dir
        assert_eq!(path1, path2, "Should reuse temp dir from pool");
    }

    #[test]
    fn test_aperture_cmd_helper() {
        let cmd = aperture_cmd();
        // This should not panic and should create a valid command
        assert!(cmd.get_program().to_string_lossy().ends_with("aperture"));
    }
}
