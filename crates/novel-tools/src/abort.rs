use tokio::sync::watch;

/// Abort signal for tool execution (mirrors novel-core InterruptReason without crate dependency).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AbortSignal {
    #[default]
    None,
    UserCancel,
    SubmitInterrupt,
    SiblingError,
    StreamingFallback,
}

impl AbortSignal {
    pub fn is_aborted(self) -> bool {
        !matches!(self, Self::None)
    }
}

pub type AbortWatch = watch::Receiver<AbortSignal>;

pub fn abort_channel() -> (watch::Sender<AbortSignal>, AbortWatch) {
    watch::channel(AbortSignal::None)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterruptBehavior {
    Cancel,
    Block,
}

pub const REJECT_MESSAGE: &str =
    "The user doesn't want to proceed with this tool use. The tool use was rejected.";
