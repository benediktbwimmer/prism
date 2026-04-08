use std::fmt;

use crate::CoordinationAuthorityStamp;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinationAuthorityMutationStatus {
    Conflict,
    Rejected,
    Indeterminate,
}

#[derive(Debug, Clone)]
pub struct CoordinationAuthorityMutationError {
    pub status: CoordinationAuthorityMutationStatus,
    pub reason_code: String,
    pub message: String,
    pub authority: Option<CoordinationAuthorityStamp>,
}

impl CoordinationAuthorityMutationError {
    pub fn conflict(
        reason_code: impl Into<String>,
        message: impl Into<String>,
        authority: Option<CoordinationAuthorityStamp>,
    ) -> Self {
        Self {
            status: CoordinationAuthorityMutationStatus::Conflict,
            reason_code: reason_code.into(),
            message: message.into(),
            authority,
        }
    }

    pub fn rejected(
        reason_code: impl Into<String>,
        message: impl Into<String>,
        authority: Option<CoordinationAuthorityStamp>,
    ) -> Self {
        Self {
            status: CoordinationAuthorityMutationStatus::Rejected,
            reason_code: reason_code.into(),
            message: message.into(),
            authority,
        }
    }

    pub fn indeterminate(
        reason_code: impl Into<String>,
        message: impl Into<String>,
        authority: Option<CoordinationAuthorityStamp>,
    ) -> Self {
        Self {
            status: CoordinationAuthorityMutationStatus::Indeterminate,
            reason_code: reason_code.into(),
            message: message.into(),
            authority,
        }
    }
}

impl fmt::Display for CoordinationAuthorityMutationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} [{}::{:?}]", self.message, self.reason_code, self.status)
    }
}

impl std::error::Error for CoordinationAuthorityMutationError {}
