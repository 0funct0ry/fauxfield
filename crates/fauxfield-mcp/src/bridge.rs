//! The `UiBridge` trait (SPEC §11.4). Defined here, implemented by `fauxfield-gui` in M1.2.
//! Deliberately minimal until then: M1.1 only fixes the crate boundary.

use std::{future::Future, pin::Pin};

use serde_json::Value;

/// A request crossing from the MCP runtime to the GPUI main thread.
#[derive(Debug, Clone)]
pub struct UiRequest {
    pub method: String,
    pub params: Value,
}

#[derive(Debug, thiserror::Error)]
pub enum UiError {
    #[error("ui thread busy")]
    Busy,
    #[error("{0}")]
    Failed(String),
}

pub type UiFuture = Pin<Box<dyn Future<Output = Result<Value, UiError>> + Send>>;

/// Implemented by the GUI; tools never touch GPUI entities off-thread.
pub trait UiBridge: Send + Sync + 'static {
    fn request(&self, request: UiRequest) -> UiFuture;
}
