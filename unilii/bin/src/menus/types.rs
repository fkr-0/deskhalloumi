#![allow(dead_code)]
// FIXME(T6): Menu lifecycle enum is the planned shared lifecycle surface pending MenuModel integration.

#[derive(Debug, Clone, PartialEq, Eq)]
#[derive(Default)]
pub enum MenuLifecycleState {
    #[default]
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

