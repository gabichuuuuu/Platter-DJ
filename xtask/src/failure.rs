//! The error type of the tool.

use std::fmt;

#[derive(Debug)]
pub enum Failure {
    /// The tool could not do its work: bad arguments, a missing file, a failed command.
    Message(String),
    /// A check ran to the end and found violations.
    Findings {
        check: &'static str,
        findings: Vec<String>,
    },
}

impl Failure {
    pub fn message(text: impl Into<String>) -> Self {
        Self::Message(text.into())
    }

    /// `Ok` when the check found nothing.
    pub fn from_findings(check: &'static str, findings: Vec<String>) -> Result<(), Self> {
        if findings.is_empty() {
            Ok(())
        } else {
            Err(Self::Findings { check, findings })
        }
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Message(text) => f.write_str(text),
            Self::Findings { check, findings } => {
                write!(f, "{check}: {} finding(s)", findings.len())?;
                for finding in findings {
                    write!(f, "\n  - {finding}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for Failure {}
