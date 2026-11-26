/// A call into the core failed. `reason` says what was being attempted and
/// why, in words fit to show.
#[derive(Debug, uniffi::Error)]
pub enum ShyError {
    Failed { reason: String },
}

impl std::fmt::Display for ShyError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Failed { reason } => formatter.write_str(reason),
        }
    }
}

impl std::error::Error for ShyError {}

impl From<anyhow::Error> for ShyError {
    fn from(error: anyhow::Error) -> Self {
        Self::Failed {
            reason: format!("{error:#}"),
        }
    }
}
