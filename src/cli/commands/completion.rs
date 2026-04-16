use crate::cli::CompletionShell;
use crate::error::Error;

pub fn execute_completion_script_command(shell: &CompletionShell) -> Result<(), Error> {
    let _ = shell;
    Err(Error::invalid_command(
        "completion",
        "completion script generation is not implemented",
    ))
}

pub fn execute_completion_runtime_command(
    shell: &CompletionShell,
    cword: usize,
    words: &[String],
) -> Result<(), Error> {
    let _ = (shell, cword, words);
    Err(Error::invalid_command(
        "completion",
        "runtime completion is not implemented",
    ))
}
