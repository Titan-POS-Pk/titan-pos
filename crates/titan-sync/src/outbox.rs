//! # Outbox Processor
//!
//! Processes the sync_outbox table and uploads entries to the Store Hub.
//!
//! ## Outbox Processing Flow
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                    Outbox Processor Flow                                │
//! │                                                                         │
//! │  ┌─────────────────────────────────────────────────────────────────┐   │
//! │  │                    sync_outbox Table                            │   │
//! │  │                                                                 │   │
//! │  │  id | entity_type | entity_id | payload | attempts | synced_at │   │
//! │  │  ───┼─────────────┼───────────┼─────────┼──────────┼───────────│   │
//! │  │  1  │ SALE        │ sale-001  │ {...}   │ 0        │ NULL      │   │
//! │  │  2  │ SALE        │ sale-002  │ {...}   │ 1        │ NULL      │   │
//! │  │  3  │ PAYMENT     │ pay-001   │ {...}   │ 0        │ NULL      │   │
//! │  └────────────────────────────┬────────────────────────────────────┘   │
//! │                               │                                         │
//! │                               ▼                                         │
//! │  ┌─────────────────────────────────────────────────────────────────┐   │
//! │  │                    OutboxProcessor                              │   │
//! │  │                                                                 │   │
//! │  │  1. Poll: SELECT * FROM sync_outbox                            │   │
//! │  │           WHERE synced_at IS NULL                              │   │
//! │  │           ORDER BY created_at LIMIT 100                        │   │
//! │  │                                                                 │   │
//! │  │  2. Batch: Group entries into OutboxBatch message              │   │
//! │  │                                                                 │   │
//! │  │  3. Send: Transport.send(OutboxBatch)                          │   │
//! │  │                                                                 │   │
//! │  │  4. Wait: Await BatchAck response                              │   │
//! │  │                                                                 │   │
//! │  │  5. Mark: UPDATE sync_outbox SET synced_at = NOW()             │   │
//! │  │           WHERE id IN (acked_ids)                              │   │
//! │  │                                                                 │   │
//! │  │  6. Retry: UPDATE sync_outbox SET attempts += 1                │   │
//! │  │            WHERE id IN (failed_ids)                            │   │
//! │  └─────────────────────────────────────────────────────────────────┘   │
//! │                                                                         │
//! │  TIMING:                                                               │
//! │  • Poll interval: 5 seconds (configurable)                             │
//! │  • Batch size: 100 entries (configurable)                              │
//! │  • Max retries: 10 (then logged and skipped)                           │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use titan_core::SyncOutboxEntry;
use titan_db::Database;

use crate::config::SyncConfig;
use crate::error::{SyncError, SyncResult};
use crate::protocol::{
    BatchAckPayload, OutboxBatchPayload, OutboxEntry, SyncMessage, SyncMessageKind,
};
use crate::transport::TransportHandle;

// =============================================================================
// Constants
// =============================================================================

/// Maximum number of retry attempts before skipping an entry.
const MAX_RETRY_ATTEMPTS: i64 = 10;

// =============================================================================
// Outbox Processor
// =============================================================================

/// Processes the sync_outbox table and uploads to the Store Hub.
pub struct OutboxProcessor {
    /// Database connection.
    db: Arc<Database>,

    /// Sync configuration.
    config: Arc<SyncConfig>,

    /// Transport handle for sending messages.
    transport: TransportHandle,

    /// Receiver for acknowledgement messages.
    ack_rx: mpsc::Receiver<SyncMessage>,

    /// Current batch sequence number.
    batch_seq: u64,

    /// Shutdown receiver.
    shutdown_rx: mpsc::Receiver<()>,
}

/// Handle for controlling the outbox processor.
#[derive(Clone)]
pub struct OutboxProcessorHandle {
    /// Shutdown sender.
    shutdown_tx: mpsc::Sender<()>,

    /// Sender for routing ack messages to the processor.
    ack_tx: mpsc::Sender<SyncMessage>,
}

impl OutboxProcessorHandle {
    /// Sends an acknowledgement message to the processor.
    pub async fn handle_ack(&self, message: SyncMessage) -> SyncResult<()> {
        self.ack_tx
            .send(message)
            .await
            .map_err(|_| SyncError::ChannelError("Ack channel closed".into()))
    }

    /// Triggers graceful shutdown.
    pub async fn shutdown(&self) -> SyncResult<()> {
        self.shutdown_tx
            .send(())
            .await
            .map_err(|_| SyncError::ChannelError("Shutdown channel closed".into()))
    }
}

impl OutboxProcessor {
    /// Creates a new outbox processor and returns a handle.
    pub fn new(
        db: Arc<Database>,
        config: Arc<SyncConfig>,
        transport: TransportHandle,
    ) -> (Self, OutboxProcessorHandle) {
        let (ack_tx, ack_rx) = mpsc::channel(100);
        let (shutdown_tx, shutdown_rx) = mpsc::channel(1);

        let processor = OutboxProcessor {
            db,
            config,
            transport,
            ack_rx,
            batch_seq: 0,
            shutdown_rx,
        };

        let handle = OutboxProcessorHandle { shutdown_tx, ack_tx };

        (processor, handle)
    }

    /// Runs the outbox processor loop.
    ///
    /// This should be spawned as a background task.
    pub async fn run(mut self) {
        info!("Outbox processor starting");

        let poll_interval = Duration::from_secs(self.config.sync.poll_interval_secs);
        let mut interval = tokio::time::interval(poll_interval);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                // Poll on interval
                _ = interval.tick() => {
                    if let Err(e) = self.process_batch().await {
                        error!(?e, "Failed to process outbox batch");
                    }
                }

                // Handle acknowledgements
                Some(msg) = self.ack_rx.recv() => {
                    if msg.kind == SyncMessageKind::BatchAck {
                        if let Err(e) = self.handle_batch_ack(msg).await {
                            error!(?e, "Failed to handle batch ack");
                        }
                    }
                }

                // Shutdown
                _ = self.shutdown_rx.recv() => {
                    info!("Outbox processor shutting down");
                    break;
                }
            }
        }

        info!("Outbox processor stopped");
    }

    /// Processes a batch of pending outbox entries.
    async fn process_batch(&mut self) -> SyncResult<()> {
        // Only process if connected
        if !self.transport.is_connected().await {
            debug!("Not connected, skipping outbox processing");
            return Ok(());
        }

        // Get pending entries
        let batch_size = self.config.sync.batch_size as u32;
        let entries = self.db.sync_outbox().get_pending(batch_size).await?;

        if entries.is_empty() {
            debug!("No pending outbox entries");
            return Ok(());
        }

        info!(count = entries.len(), "Processing outbox batch");

        // Filter out entries that have exceeded max retries
        let (processable, skipped): (Vec<_>, Vec<_>) =
            entries.into_iter().partition(|e| e.attempts < MAX_RETRY_ATTEMPTS);

        // Log skipped entries
        for entry in skipped {
            warn!(
                id = %entry.id,
                entity_type = %entry.entity_type,
                entity_id = %entry.entity_id,
                attempts = entry.attempts,
                "Skipping entry that exceeded max retry attempts"
            );
        }

        if processable.is_empty() {
            return Ok(());
        }

        // Build batch message
        let batch = self.build_batch(&processable)?;

        // Send batch
        let message = SyncMessage::new(SyncMessageKind::OutboxBatch, batch)?;
        self.transport.send(message).await?;

        debug!(
            count = processable.len(),
            batch_seq = self.batch_seq,
            "Sent outbox batch"
        );

        self.batch_seq += 1;

        Ok(())
    }

    /// Builds an OutboxBatchPayload from entries.
    fn build_batch(&self, entries: &[SyncOutboxEntry]) -> SyncResult<OutboxBatchPayload> {
        let batch_entries: Vec<OutboxEntry> = entries
            .iter()
            .map(|e| OutboxEntry {
                id: e.id.clone(),
                entity_type: e.entity_type.clone(),
                entity_id: e.entity_id.clone(),
                payload: e.payload.clone(),
                created_at: e.created_at,
            })
            .collect();

        Ok(OutboxBatchPayload {
            device_id: self.config.device.id.clone(),
            entries: batch_entries,
            batch_seq: self.batch_seq,
        })
    }

    /// Handles a batch acknowledgement.
    async fn handle_batch_ack(&self, message: SyncMessage) -> SyncResult<()> {
        let ack: BatchAckPayload = message.extract_payload()?;

        info!(
            acked = ack.acked_ids.len(),
            failed = ack.failed_ids.len(),
            new_cursor = ack.new_cursor,
            "Received batch acknowledgement"
        );

        // Mark acked entries as synced
        for id in &ack.acked_ids {
            if let Err(e) = self.db.sync_outbox().mark_synced(id).await {
                error!(?e, id = %id, "Failed to mark entry as synced");
            }
        }

        // Mark failed entries with error
        for failed in &ack.failed_ids {
            let error_msg = format!(
                "Sync failed: {} (retryable: {})",
                failed.error, failed.retryable
            );

            if let Err(e) = self.db.sync_outbox().mark_failed(&failed.id, &error_msg).await {
                error!(?e, id = %failed.id, "Failed to mark entry as failed");
            }

            if !failed.retryable {
                warn!(
                    id = %failed.id,
                    error = %failed.error,
                    "Non-retryable sync failure"
                );
            }
        }

        Ok(())
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_max_retry_constant() {
        assert_eq!(MAX_RETRY_ATTEMPTS, 10);
    }
}
