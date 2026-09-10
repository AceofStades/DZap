// Port of server-go/realtime/hub.go
use tokio::sync::broadcast;

/// Broadcast hub for websocket clients. Unlike the Go channel-based hub,
/// `tokio::sync::broadcast` handles registration/unregistration internally.
#[derive(Clone)]
pub struct Hub {
    pub sender: broadcast::Sender<String>,
}

impl Hub {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(256);
        Hub { sender }
    }

    pub fn broadcast(&self, message: String) {
        // Ignore errors: they only mean there are no connected clients.
        let _ = self.sender.send(message);
    }
}

impl Default for Hub {
    fn default() -> Self {
        Self::new()
    }
}
