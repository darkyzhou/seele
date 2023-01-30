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

impl Into<&'static str> for OjStatus {
    fn into(self) -> &'static str {
        match self {
            Self::Accepted => "ACC",
            Self::TimeLimitExceeded => "TLE",
            Self::MemoryLimitExceeded => "MLE",
            Self::WrongAnswer => "WA",
            Self::RuntimeError => "RE",
            Self::OutputLimitExceeded => "OLE",
            Self::InternalError => "INTERNAL",
        }
    }
}

pub fn get_oj_status(run_report: ExecutionReport, compare_report: ExecutionReport) -> OjStatus {
    if matches!(run_report.status, ExecutionStatus::Normal)
        && matches!(compare_report.status, ExecutionStatus::RuntimeError)
    {
        return OjStatus::WrongAnswer;
    }

    if !matches!(run_report.status, ExecutionStatus::Normal) {
        return OjStatus::RuntimeError;
    }

    if matches!(
        (&run_report.status, &compare_report.status),
        (&ExecutionStatus::Normal, &ExecutionStatus::Normal)
    ) {
        return OjStatus::Accepted;
    }

    if run_report.is_wall_tle || run_report.is_user_tle || run_report.is_system_tle {
        return OjStatus::TimeLimitExceeded;
    }

    if matches!(run_report.status, ExecutionStatus::OutputLimitExceeded) {
        return OjStatus::OutputLimitExceeded;
    }

    if run_report.is_oom {
        return OjStatus::MemoryLimitExceeded;
    }

    return OjStatus::InternalError;
}
