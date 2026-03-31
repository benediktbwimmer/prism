use std::error::Error;
use std::fmt::{self, Display, Formatter};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmissionBusyError {
    code: &'static str,
    resource: &'static str,
    operation: &'static str,
    next_action: &'static str,
}

impl AdmissionBusyError {
    pub fn refresh_lock(operation: &'static str) -> Self {
        Self {
            code: "runtime_admission_busy",
            resource: "refresh_lock",
            operation,
            next_action: "Retry the request shortly; the workspace refresh critical section is currently busy.",
        }
    }

    pub fn runtime_sync(operation: &'static str) -> Self {
        Self {
            code: "runtime_admission_busy",
            resource: "workspace_runtime_sync_lock",
            operation,
            next_action:
                "Retry the request shortly; the workspace runtime sync path is currently busy.",
        }
    }

    pub fn code(&self) -> &'static str {
        self.code
    }

    pub fn resource(&self) -> &'static str {
        self.resource
    }

    pub fn operation(&self) -> &'static str {
        self.operation
    }

    pub fn next_action(&self) -> &'static str {
        self.next_action
    }
}

impl Display for AdmissionBusyError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "request admission busy for `{}`: `{}` is currently held",
            self.operation, self.resource
        )
    }
}

impl Error for AdmissionBusyError {}
