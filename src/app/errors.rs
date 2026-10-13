use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum AppError {
    Search(String),
    Mix(String),
    Playback(String),
    Jam(String),
    Ui(String),
}

impl fmt::Display for AppError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Search(message) => write!(formatter, "Search failed: {message}"),
            Self::Mix(message) => write!(formatter, "Mix failed: {message}"),
            Self::Playback(message) => write!(formatter, "Playback failed: {message}"),
            Self::Jam(message) => write!(formatter, "Jam failed: {message}"),
            Self::Ui(message) => formatter.write_str(message),
        }
    }
}
