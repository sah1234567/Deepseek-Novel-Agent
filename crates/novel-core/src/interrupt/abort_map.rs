//! `interrupt/abort_map` — LLM abort 信号映射。

use crate::InterruptReason;
use novel_tools::AbortSignal;

pub(crate) fn map_abort_signal(reason: Option<InterruptReason>) -> AbortSignal {
    match reason {
        Some(InterruptReason::UserCancel) => AbortSignal::UserCancel,
        Some(InterruptReason::SubmitInterrupt) => AbortSignal::SubmitInterrupt,
        Some(InterruptReason::SiblingError) => AbortSignal::SiblingError,
        Some(InterruptReason::StreamingFallback) => AbortSignal::StreamingFallback,
        None => AbortSignal::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case(Some(InterruptReason::UserCancel), AbortSignal::UserCancel)]
    #[case(Some(InterruptReason::SubmitInterrupt), AbortSignal::SubmitInterrupt)]
    #[case(Some(InterruptReason::SiblingError), AbortSignal::SiblingError)]
    #[case(
        Some(InterruptReason::StreamingFallback),
        AbortSignal::StreamingFallback
    )]
    #[case(None, AbortSignal::None)]
    fn map_abort_signal_cases(
        #[case] reason: Option<InterruptReason>,
        #[case] expected: AbortSignal,
    ) {
        assert_eq!(map_abort_signal(reason), expected);
    }
}
