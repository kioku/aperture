use crate::error::Error;
use std::time::Duration;

#[cfg(test)]
use mockall::predicate::*;
#[cfg(test)]
use mockall::*;

/// Trait abstraction for input/output operations to enable mocking
#[cfg_attr(test, mockall::automock)]
pub trait InputOutput {
    /// Print text to output
    fn print(&self, text: &str) -> Result<(), Error>;
    
    /// Print text to output with newline
    fn println(&self, text: &str) -> Result<(), Error>;
    
    /// Flush output buffer
    fn flush(&self) -> Result<(), Error>;
    
    /// Read a line of input from user
    fn read_line(&self) -> Result<String, Error>;
    
    /// Read a line of input from user with timeout
    fn read_line_with_timeout(&self, timeout: Duration) -> Result<String, Error>;
}

/// Real implementation of InputOutput that uses stdin/stdout
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
        std::io::stdout().flush().map_err(Error::Io)
    }
    
    fn read_line(&self) -> Result<String, Error> {
        use std::io::BufRead;
        let stdin = std::io::stdin();
        let mut line = String::new();
        stdin.lock().read_line(&mut line).map_err(Error::Io)?;
        Ok(line)
    }
    
    fn read_line_with_timeout(&self, timeout: Duration) -> Result<String, Error> {
        use std::sync::mpsc;
        use std::thread;
        use std::io::BufRead;
        
        let (tx, rx) = mpsc::channel();
        
        // Spawn a thread to read from stdin
        let read_thread = thread::spawn(move || {
            let stdin = std::io::stdin();
            let mut line = String::new();
            match stdin.lock().read_line(&mut line) {
                Ok(_) => tx.send(Ok(line)).unwrap_or(()),
                Err(e) => tx.send(Err(Error::Io(e))).unwrap_or(()),
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
                // Note: The thread will continue running until user provides input
                // This is a limitation of stdin reading in Rust - we can't easily cancel it
                Err(Error::InvalidConfig {
                    reason: format!("Input timeout after {} seconds", timeout.as_secs()),
                })
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err(Error::InvalidConfig {
                    reason: "Input channel disconnected".to_string(),
                })
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