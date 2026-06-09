use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::watch;

/// Why the current turn was aborted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterruptReason {
    UserCancel,
    SubmitInterrupt,
    SiblingError,
    StreamingFallback,
}

impl InterruptReason {
    pub fn parse_reason(s: &str) -> Self {
        match s {
            "interrupt" | "submit-interrupt" => Self::SubmitInterrupt,
            "sibling_error" => Self::SiblingError,
            "streaming_fallback" => Self::StreamingFallback,
            _ => Self::UserCancel,
        }
    }
}

impl std::str::FromStr for InterruptReason {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::parse_reason(s))
    }
}

pub const ERROR_MESSAGE_USER_ABORT: &str = "API Error: Request was aborted.";

/// Shared abort controller for the active turn.
#[derive(Clone)]
pub struct AbortController {
    tx: watch::Sender<Option<InterruptReason>>,
    rx: watch::Receiver<Option<InterruptReason>>,
    /// Fast-path flag for LLM stream select loops.
    flag: Arc<AtomicBool>,
}

impl AbortController {
    pub fn new() -> Self {
        let (tx, rx) = watch::channel(None);
        Self {
            tx,
            rx,
            flag: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn shared() -> Arc<Self> {
        Arc::new(Self::new())
    }

    pub fn request(&self, reason: InterruptReason) {
        self.flag.store(true, Ordering::SeqCst);
        let _ = self.tx.send(Some(reason));
    }

    pub fn clear(&self) {
        self.flag.store(false, Ordering::SeqCst);
        let _ = self.tx.send(None);
    }

    pub fn is_aborted(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }

    pub fn reason(&self) -> Option<InterruptReason> {
        *self.rx.borrow()
    }

    pub fn subscribe(&self) -> watch::Receiver<Option<InterruptReason>> {
        self.rx.clone()
    }

    pub fn cancel_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.flag)
    }
}

impl Default for AbortController {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_and_clear() {
        let ac = AbortController::new();
        assert!(!ac.is_aborted());
        ac.request(InterruptReason::UserCancel);
        assert!(ac.is_aborted());
        assert_eq!(ac.reason(), Some(InterruptReason::UserCancel));
        ac.clear();
        assert!(!ac.is_aborted());
    }

    #[test]
    fn parse_reason_strings() {
        assert_eq!(
            "interrupt".parse::<InterruptReason>().unwrap(),
            InterruptReason::SubmitInterrupt
        );
        assert_eq!(
            "user-cancel".parse::<InterruptReason>().unwrap(),
            InterruptReason::UserCancel
        );
    }
}
