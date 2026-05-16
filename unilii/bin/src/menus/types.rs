#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuLifecycleState {
    Closed,
    Opening,
    Ready,
    Busy {
        action_id: String,
    },
    Error {
        scope: String,
        message: String,
        recoverable: bool,
    },
    Stale,
}

impl Default for MenuLifecycleState {
    fn default() -> Self {
        Self::Closed
    }
}
