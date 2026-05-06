//! Mock feedback module for tests
#![allow(dead_code)]

pub struct CodexFeedback {
    _private: (),
}

impl CodexFeedback {
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl Default for CodexFeedback {
    fn default() -> Self {
        Self::new()
    }
}
