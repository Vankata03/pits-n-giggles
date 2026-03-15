use std::error::Error;
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum TelemetryParseError {
    MissingField(&'static str),
    MissingAnyField(&'static [&'static str]),
    InvalidField(&'static str),
}

impl fmt::Display for TelemetryParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingField(field) => write!(f, "missing telemetry field '{field}'"),
            Self::MissingAnyField(fields) => write!(
                f,
                "missing telemetry field, expected one of: {}",
                fields.join(", ")
            ),
            Self::InvalidField(field) => write!(f, "invalid telemetry field '{field}'"),
        }
    }
}

impl Error for TelemetryParseError {}

#[derive(Debug)]
pub enum FetchTelemetryError {
    Http(reqwest::Error),
    Parse(TelemetryParseError),
}

impl fmt::Display for FetchTelemetryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Http(error) => write!(f, "failed to fetch stream overlay data: {error}"),
            Self::Parse(error) => write!(f, "failed to parse stream overlay data: {error}"),
        }
    }
}

impl Error for FetchTelemetryError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Http(error) => Some(error),
            Self::Parse(error) => Some(error),
        }
    }
}
