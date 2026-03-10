/// Errors that can occur during HTTP request building or response parsing.
#[derive(Debug)]
pub enum HttpError {
    /// Failed to deserialize the response body or headers.
    Deserialization,
    /// General failure when parsing HTTP response structure.
    GenericParseError,
    /// A heapless buffer exceeded its fixed capacity.
    BufferOverflow,
}

impl core::fmt::Display for HttpError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            HttpError::Deserialization => write!(f, "Failed to deserialize response"),
            HttpError::GenericParseError => write!(f, "Failed to parse response"),
            HttpError::BufferOverflow => write!(f, "Buffer capacity exceeded"),
        }
    }
}
