use std::fmt;

/// Central error type for the CLI.
#[derive(Debug)]
pub enum CliError {
    /// A subprocess exited with a non-zero code.
    Command {
        cmd: String,
        code: i32,
        stderr: String,
    },
    /// A wait/poll operation exceeded its deadline.
    Timeout { message: String },
    /// Generic I/O failure.
    Io(std::io::Error),
    /// HTTP request returned an unexpected status.
    Http { status: u16, body: String },
    /// Bad or missing configuration.
    Config(String),
    /// One or more required CLI tools are missing.
    MissingTools(Vec<String>),
    /// Docker daemon is not running.
    DockerNotRunning,
    /// Ctrl-C or similar signal.
    Interrupted,
    /// User answered "no" to a confirmation prompt.
    UserAborted,
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Command { cmd, code, stderr } => {
                write!(f, "command `{}` exited with code {}", cmd, code)?;
                if !stderr.is_empty() {
                    write!(f, ": {}", stderr)?;
                }
                Ok(())
            }
            Self::Timeout { message } => write!(f, "timeout: {}", message),
            Self::Io(e) => write!(f, "I/O error: {}", e),
            Self::Http { status, body } => {
                write!(f, "HTTP {}", status)?;
                if !body.is_empty() {
                    write!(f, ": {}", body)?;
                }
                Ok(())
            }
            Self::Config(msg) => write!(f, "config error: {}", msg),
            Self::MissingTools(tools) => {
                write!(f, "missing required tools: {}", tools.join(", "))
            }
            Self::DockerNotRunning => write!(f, "Docker daemon is not running"),
            Self::Interrupted => write!(f, "interrupted"),
            Self::UserAborted => write!(f, "aborted by user"),
        }
    }
}

impl std::error::Error for CliError {}

impl From<std::io::Error> for CliError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
