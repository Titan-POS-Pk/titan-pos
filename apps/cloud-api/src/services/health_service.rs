//! Health check gRPC service implementation.
//!
//! Provides health checks for monitoring and keepalive.

use std::pin::Pin;
use std::sync::Arc;

use chrono::Utc;
use tokio::sync::mpsc;
use tokio::time::{interval, Duration};
use tokio_stream::{wrappers::ReceiverStream, Stream};
use tonic::{Request, Response, Status};
use tracing::info;

use crate::proto::{
    health_service_server::HealthService,
    health_check_response::ServingStatus,
    HealthCheckRequest, HealthCheckResponse,
    Timestamp as ProtoTimestamp,
};
use crate::AppState;

/// Health check interval for watch stream.
const HEALTH_CHECK_INTERVAL: Duration = Duration::from_secs(10);

/// Health service implementation.
pub struct HealthServiceImpl {
    state: Arc<AppState>,
}

impl HealthServiceImpl {
    /// Create a new health service.
    pub fn new(state: Arc<AppState>) -> Self {
        HealthServiceImpl { state }
    }

    /// Check the health of a specific service or overall system.
    async fn check_health(&self, service: &str) -> HealthCheckResponse {
        let status = match service {
            "" | "overall" => {
                // Check overall system health
                self.check_overall_health().await
            }
            "database" => {
                // Check database connectivity
                self.check_database_health().await
            }
            "redis" => {
                // Check Redis connectivity
                self.check_redis_health().await
            }
            _ => {
                // Unknown service
                (ServingStatus::Unknown, format!("Unknown service: {}", service))
            }
        };

        HealthCheckResponse {
            status: status.0 as i32,
            message: status.1,
            server_time: Some(ProtoTimestamp {
                value: Utc::now().to_rfc3339(),
            }),
        }
    }

    /// Check overall system health.
    async fn check_overall_health(&self) -> (ServingStatus, String) {
        // Check database
        let db_health = self.check_database_health().await;
        if db_health.0 != ServingStatus::Serving {
            return (ServingStatus::NotServing, format!("Database unhealthy: {}", db_health.1));
        }

        // Check Redis (optional)
        if self.state.redis.is_some() {
            let redis_health = self.check_redis_health().await;
            if redis_health.0 != ServingStatus::Serving {
                // Redis is optional, so we're degraded but not down
                return (ServingStatus::Serving, format!("Degraded: Redis unhealthy - {}", redis_health.1));
            }
        }

        (ServingStatus::Serving, "All systems operational".to_string())
    }

    /// Check database health.
    async fn check_database_health(&self) -> (ServingStatus, String) {
        match sqlx::query("SELECT 1")
            .fetch_one(self.state.db.pool())
            .await
        {
            Ok(_) => (ServingStatus::Serving, "Database connected".to_string()),
            Err(e) => (ServingStatus::NotServing, format!("Database error: {}", e)),
        }
    }

    /// Check Redis health.
    async fn check_redis_health(&self) -> (ServingStatus, String) {
        match &self.state.redis {
            Some(client) => {
                match client.get_connection() {
                    Ok(mut conn) => {
                        match redis::cmd("PING").query::<String>(&mut conn) {
                            Ok(_) => (ServingStatus::Serving, "Redis connected".to_string()),
                            Err(e) => (ServingStatus::NotServing, format!("Redis ping failed: {}", e)),
                        }
                    }
                    Err(e) => (ServingStatus::NotServing, format!("Redis connection failed: {}", e)),
                }
            }
            None => (ServingStatus::Unknown, "Redis not configured".to_string()),
        }
    }
}

#[tonic::async_trait]
impl HealthService for HealthServiceImpl {
    /// Simple health check.
    async fn check(
        &self,
        request: Request<HealthCheckRequest>,
    ) -> Result<Response<HealthCheckResponse>, Status> {
        let req = request.into_inner();
        let response = self.check_health(&req.service).await;
        Ok(Response::new(response))
    }

    type WatchStream = Pin<Box<dyn Stream<Item = Result<HealthCheckResponse, Status>> + Send>>;

    /// Streaming health check (keepalive).
    async fn watch(
        &self,
        request: Request<HealthCheckRequest>,
    ) -> Result<Response<Self::WatchStream>, Status> {
        let req = request.into_inner();
        let service = req.service;
        let state = self.state.clone();

        info!(service = %service, "Starting health watch stream");

        let (tx, rx) = mpsc::channel(16);

        tokio::spawn(async move {
            let health_service = HealthServiceImpl { state };
            let mut check_interval = interval(HEALTH_CHECK_INTERVAL);

            loop {
                check_interval.tick().await;
                
                let response = health_service.check_health(&service).await;
                
                if tx.send(Ok(response)).await.is_err() {
                    // Client disconnected
                    break;
                }
            }

            info!(service = %service, "Health watch stream ended");
        });

        let output_stream = ReceiverStream::new(rx);
        Ok(Response::new(Box::pin(output_stream)))
    }
}
