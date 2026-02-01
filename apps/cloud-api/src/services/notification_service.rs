//! Notification gRPC service implementation.
//!
//! Provides server-push notifications via bidirectional streaming.

use std::pin::Pin;
use std::sync::Arc;

use chrono::Utc;
use tokio::sync::mpsc;
use tokio::time::{interval, Duration};
use tokio_stream::{wrappers::ReceiverStream, Stream, StreamExt};
use tonic::{Request, Response, Status, Streaming};
use tracing::{debug, info, warn};

use crate::auth::{extract_bearer_token, JwtManager};
use crate::proto::{
    notification_service_server::NotificationService,
    HeartbeatNotification, Notification, SubscriptionMessage,
    Timestamp as ProtoTimestamp,
};
use crate::AppState;

/// Heartbeat interval for keeping connections alive.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);

/// Notification service implementation.
pub struct NotificationServiceImpl {
    state: Arc<AppState>,
    jwt_manager: JwtManager,
}

impl NotificationServiceImpl {
    /// Create a new notification service.
    pub fn new(state: Arc<AppState>) -> Self {
        let jwt_manager = JwtManager::new(
            state.config.jwt_secret.clone(),
            state.config.jwt_access_lifetime_secs,
            state.config.jwt_refresh_lifetime_secs,
        );
        
        NotificationServiceImpl { state, jwt_manager }
    }

    /// Authenticate a subscription request.
    fn authenticate_stream(&self, request: &Request<Streaming<SubscriptionMessage>>) -> Result<String, Status> {
        let auth_header = request
            .metadata()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| Status::unauthenticated("Missing authorization header"))?;

        let token = extract_bearer_token(auth_header)
            .ok_or_else(|| Status::unauthenticated("Invalid authorization header"))?;

        let claims = self.jwt_manager
            .validate_access_token(token)
            .map_err(|e| Status::unauthenticated(e.to_string()))?;

        Ok(claims.sub)
    }
}

#[tonic::async_trait]
impl NotificationService for NotificationServiceImpl {
    type SubscribeStream = Pin<Box<dyn Stream<Item = Result<Notification, Status>> + Send>>;

    /// Subscribe to real-time notifications.
    async fn subscribe(
        &self,
        request: Request<Streaming<SubscriptionMessage>>,
    ) -> Result<Response<Self::SubscribeStream>, Status> {
        let store_id = self.authenticate_stream(&request)?;
        let mut inbound = request.into_inner();

        info!(store_id = %store_id, "New notification subscription");

        let (tx, rx) = mpsc::channel(64);
        let state = self.state.clone();

        // Spawn task to handle the subscription
        tokio::spawn(async move {
            let mut heartbeat_interval = interval(HEARTBEAT_INTERVAL);
            let mut notification_counter: u64 = 0;
            let mut subscribed_topics: Vec<String> = Vec::new();

            loop {
                tokio::select! {
                    // Handle incoming messages from client
                    Some(result) = inbound.next() => {
                        match result {
                            Ok(msg) => {
                                debug!(
                                    store_id = %store_id,
                                    topics = ?msg.topics,
                                    "Subscription update"
                                );

                                // Update subscribed topics
                                if !msg.topics.is_empty() {
                                    subscribed_topics = msg.topics;
                                }

                                // Client acknowledged heartbeat
                                if msg.heartbeat_ack {
                                    debug!(store_id = %store_id, "Heartbeat acknowledged");
                                }
                            }
                            Err(e) => {
                                warn!(store_id = %store_id, ?e, "Subscription error");
                                break;
                            }
                        }
                    }

                    // Send periodic heartbeats
                    _ = heartbeat_interval.tick() => {
                        notification_counter += 1;
                        let notification = Notification {
                            notification_id: format!("hb-{}-{}", store_id, notification_counter),
                            topic: "HEARTBEAT".to_string(),
                            timestamp: Some(ProtoTimestamp {
                                value: Utc::now().to_rfc3339(),
                            }),
                            payload: Some(crate::proto::notification::Payload::Heartbeat(
                                HeartbeatNotification {
                                    server_time: Some(ProtoTimestamp {
                                        value: Utc::now().to_rfc3339(),
                                    }),
                                },
                            )),
                        };

                        if tx.send(Ok(notification)).await.is_err() {
                            debug!(store_id = %store_id, "Subscription channel closed");
                            break;
                        }
                    }
                }
            }

            info!(store_id = %store_id, "Notification subscription ended");
        });

        let output_stream = ReceiverStream::new(rx);
        Ok(Response::new(Box::pin(output_stream)))
    }
}
