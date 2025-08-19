use crate::error::Error;
use std::time::Duration;

#[cfg(test)]
use mockall::predicate::*;

/// Trait abstraction for input/output operations to enable mocking
#[cfg_attr(test, mockall::automock)]
pub trait InputOutput {
    /// Print text to output
    ///
    /// # Errors
    /// Returns an error if the output operation fails
    fn print(&self, text: &str) -> Result<(), Error>;

    /// Print text to output with newline
    ///
    /// # Errors
    /// Returns an error if the output operation fails
    fn println(&self, text: &str) -> Result<(), Error>;

    /// Flush output buffer
    ///
    /// # Errors
    /// Returns an error if the flush operation fails
    fn flush(&self) -> Result<(), Error>;

    /// Read a line of input from user
    ///
    /// # Errors
    /// Returns an error if the input operation fails
    fn read_line(&self) -> Result<String, Error>;

    /// Read a line of input from user with timeout
    ///
    /// # Errors
    /// Returns an error if the input operation fails or times out
    fn read_line_with_timeout(&self, timeout: Duration) -> Result<String, Error>;
}

/// Real implementation of `InputOutput` that uses stdin/stdout
pub struct RealInputOutput;

impl InputOutput for RealInputOutput {
    fn print(&self, text: &str) -> Result<(), Error> {
        print!("{text}");
        Ok(())
    }

    fn println(&self, text: &str) -> Result<(), Error> {
        println!("{text}");
        Ok(())
    }

    fn flush(&self) -> Result<(), Error> {
        use std::io::Write;
        std::io::stdout()
            .flush()
            .map_err(|e| Error::io_error(format!("Failed to flush stdout: {e}")))
    }

    fn read_line(&self) -> Result<String, Error> {
        use std::io::BufRead;
        let stdin = std::io::stdin();
        let mut line = String::new();
        stdin
            .lock()
            .read_line(&mut line)
            .map_err(|e| Error::io_error(format!("Failed to read from stdin: {e}")))?;
        Ok(line)
    }

    fn read_line_with_timeout(&self, timeout: Duration) -> Result<String, Error> {
        use std::io::BufRead;
        use std::sync::mpsc;
        use std::thread;

        let (tx, rx) = mpsc::channel();

        // Spawn a thread to read from stdin
        // Note: This thread will continue running even after timeout due to blocking stdin read
        let read_thread = thread::spawn(move || {
            let stdin = std::io::stdin();
            let mut line = String::new();
            let result = stdin.lock().read_line(&mut line);
            match result {
                Ok(_) => tx.send(Ok(line)).unwrap_or(()),
                Err(e) => tx
                    .send(Err(Error::io_error(format!(
                        "Failed to read from stdin: {e}"
                    ))))
                    .unwrap_or(()),
            }
        });

        // Wait for either input or timeout
        match rx.recv_timeout(timeout) {
            Ok(result) => {
                // Join the thread to clean up
                let _ = read_thread.join();
                result
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Important: The read thread will continue running until user provides input.
                // This is a known limitation of stdin reading in Rust - blocking reads cannot
                // be cancelled. The thread will eventually clean up when:
                // 1. The user provides input (thread completes normally)
                // 2. The process exits (OS cleans up all threads)
                // This is the standard approach for stdin timeout handling in Rust.
                Err(Error::interactive_timeout())
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err(Error::invalid_config("Input channel disconnected"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_input_output() {
        let mut mock = MockInputOutput::new();

        // Set up expectations
        mock.expect_print()
            .with(eq("Hello"))
            .times(1)
            .returning(|_| Ok(()));

        mock.expect_read_line()
            .times(1)
            .returning(|| Ok("test input\n".to_string()));

        // Test the mock
        assert!(mock.print("Hello").is_ok());
        assert_eq!(mock.read_line().unwrap(), "test input\n");
    }
}
