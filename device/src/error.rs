#[derive(Debug)]
pub enum HttpError {
    Deserialization,
    GenericParseError,
}

impl core::fmt::Display for HttpError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            HttpError::Deserialization => write!(f, "Failed to deserialize response"),
            HttpError::GenericParseError => write!(f, "Failed to parse response"),
        }
    }
}
