use crate::realtime::Hub;

#[tokio::test]
async fn broadcast_reaches_every_current_subscriber() {
    let hub = Hub::new();
    let mut first = hub.sender.subscribe();
    let mut second = hub.sender.subscribe();

    hub.broadcast("progress".to_string());

    assert_eq!(first.recv().await.unwrap(), "progress");
    assert_eq!(second.recv().await.unwrap(), "progress");
}

#[test]
fn broadcast_without_subscribers_is_harmless() {
    Hub::default().broadcast("nobody listening".to_string());
}
