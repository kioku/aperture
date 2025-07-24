use crate::error::Error;

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