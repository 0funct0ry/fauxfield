//! `GuiBridge`: the channel from the MCP runtime into the GPUI thread (SPEC §3.4). Each request
//! carries a oneshot reply and is answered within five seconds or fails with "ui thread busy".

use std::time::Duration;

use fauxfield_mcp::{UiBridge, UiError, UiRequest, bridge::UiFuture};
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};

pub const REPLY_TIMEOUT: Duration = Duration::from_secs(5);

pub type Reply = oneshot::Sender<Result<Value, UiError>>;
pub type Envelope = (UiRequest, Reply);

pub struct GuiBridge {
    tx: mpsc::UnboundedSender<Envelope>,
    timeout: Duration,
}

impl GuiBridge {
    /// The bridge and the receiver the GPUI thread drains.
    pub fn channel() -> (GuiBridge, mpsc::UnboundedReceiver<Envelope>) {
        Self::with_timeout(REPLY_TIMEOUT)
    }

    pub fn with_timeout(timeout: Duration) -> (GuiBridge, mpsc::UnboundedReceiver<Envelope>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (GuiBridge { tx, timeout }, rx)
    }
}

impl UiBridge for GuiBridge {
    fn request(&self, request: UiRequest) -> UiFuture {
        let tx = self.tx.clone();
        let timeout = self.timeout;
        Box::pin(async move {
            let (reply, rx) = oneshot::channel();
            tx.send((request, reply))
                .map_err(|_| UiError::Failed("the app is shutting down".into()))?;
            match tokio::time::timeout(timeout, rx).await {
                Ok(Ok(result)) => result,
                Ok(Err(_)) => Err(UiError::Failed("the ui dropped the request".into())),
                Err(_) => Err(UiError::Busy),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn req(method: &str) -> UiRequest {
        UiRequest {
            method: method.into(),
            params: json!({}),
        }
    }

    #[tokio::test]
    async fn unanswered_request_is_busy() {
        let (bridge, _rx) = GuiBridge::with_timeout(Duration::from_millis(50));
        let err = bridge.request(req("ui.state")).await.unwrap_err();
        assert!(matches!(err, UiError::Busy));
        assert_eq!(err.to_string(), "ui thread busy");
    }

    #[tokio::test]
    async fn answered_request_returns_the_reply() {
        let (bridge, mut rx) = GuiBridge::channel();
        tokio::spawn(async move {
            let (r, reply) = rx.recv().await.unwrap();
            reply.send(Ok(json!({ "echo": r.method }))).unwrap();
        });
        let v = bridge.request(req("ui.state")).await.unwrap();
        assert_eq!(v, json!({ "echo": "ui.state" }));
    }

    #[tokio::test]
    async fn closed_receiver_is_a_failure_not_a_hang() {
        let (bridge, rx) = GuiBridge::channel();
        drop(rx);
        assert!(matches!(
            bridge.request(req("ui.state")).await,
            Err(UiError::Failed(_))
        ));
    }
}
