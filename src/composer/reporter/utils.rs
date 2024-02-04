use crate::worker::run_container::{ExecutionReport, ExecutionStatus};

pub enum OjStatus {
    Accepted,
    TimeLimitExceeded,
    MemoryLimitExceeded,
    WrongAnswer,
    RuntimeError,
    OutputLimitExceeded,
    InternalError,
}

impl From<OjStatus> for &'static str {
    fn from(status: OjStatus) -> Self {
        match status {
            OjStatus::Accepted => "ACC",
            OjStatus::TimeLimitExceeded => "TLE",
            OjStatus::MemoryLimitExceeded => "MLE",
            OjStatus::WrongAnswer => "WA",
            OjStatus::RuntimeError => "RE",
            OjStatus::OutputLimitExceeded => "OLE",
            OjStatus::InternalError => "INTERNAL",
        }
    }
}

pub fn get_oj_status(run_report: ExecutionReport, compare_report: ExecutionReport) -> OjStatus {
    if matches!(run_report.status, ExecutionStatus::Normal)
        && matches!(compare_report.status, ExecutionStatus::RuntimeError)
    {
        return OjStatus::WrongAnswer;
    }

    if matches!(
        (&run_report.status, &compare_report.status),
        (&ExecutionStatus::Normal, &ExecutionStatus::Normal)
    ) {
        return OjStatus::Accepted;
    }

    if matches!(
        run_report.status,
        ExecutionStatus::UserTimeLimitExceeded | ExecutionStatus::WallTimeLimitExceeded
    ) {
        return OjStatus::TimeLimitExceeded;
    }

    if matches!(run_report.status, ExecutionStatus::OutputLimitExceeded) {
        return OjStatus::OutputLimitExceeded;
    }

    if matches!(run_report.status, ExecutionStatus::MemoryLimitExceeded) {
        return OjStatus::MemoryLimitExceeded;
    }

    if !matches!(run_report.status, ExecutionStatus::Normal) {
        return OjStatus::RuntimeError;
    }

    OjStatus::InternalError
}
