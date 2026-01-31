//! # Sync Outbox Repository
//!
//! Manages the sync outbox queue for offline-first synchronization.
//!
//! ## The Outbox Pattern
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                    Outbox Pattern Implementation                        │
//! │                                                                         │
//! │  LOCAL OPERATION (e.g., finalize_sale)                                 │
//! │       │                                                                 │
//! │       ▼                                                                 │
//! │  ┌─────────────────────────────────────────────────────────────────┐   │
//! │  │                   SINGLE TRANSACTION                            │   │
//! │  │                                                                 │   │
//! │  │  1. UPDATE sales SET status = 'completed' WHERE id = ?         │   │
//! │  │                                                                 │   │
//! │  │  2. INSERT INTO sync_outbox (entity_type, entity_id, payload)  │   │
//! │  │     VALUES ('SALE', ?, <full sale JSON>)                       │   │
//! │  │                                                                 │   │
//! │  └─────────────────────────────────────────────────────────────────┘   │
//! │       │                                                                 │
//! │       ▼                                                                 │
//! │  COMMIT ← Both succeed or both fail (atomicity guaranteed)             │
//! │       │                                                                 │
//! │       ▼                                                                 │
//! │  ┌─────────────────────────────────────────────────────────────────┐   │
//! │  │            BACKGROUND SYNC WORKER (async)                       │   │
//! │  │                                                                 │   │
//! │  │  1. SELECT * FROM sync_outbox WHERE synced_at IS NULL          │   │
//! │  │                                                                 │   │
//! │  │  2. For each entry:                                            │   │
//! │  │     a. Send to cloud API                                       │   │
//! │  │     b. On success: UPDATE sync_outbox SET synced_at = NOW()    │   │
//! │  │     c. On failure: UPDATE sync_outbox SET attempts += 1,       │   │
//! │  │                    last_error = ?                              │   │
//! │  │                                                                 │   │
//! │  └─────────────────────────────────────────────────────────────────┘   │
//! │                                                                         │
//! │  KEY GUARANTEES:                                                       │
//! │  • Sale is never lost (it's in local DB)                               │
//! │  • Sync entry is never orphaned (same transaction)                     │
//! │  • Offline? No problem - entries queue up                              │
//! │  • Back online? Worker syncs pending entries                           │
//! │                                                                         │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```

use chrono::Utc;
use sqlx::SqlitePool;
use tracing::debug;
use uuid::Uuid;

use crate::error::DbResult;
use titan_core::{SyncOutboxEntry, DEFAULT_TENANT_ID};

/// Repository for sync outbox operations.
#[derive(Debug, Clone)]
pub struct SyncOutboxRepository {
    pool: SqlitePool,
}

impl SyncOutboxRepository {
    /// Creates a new SyncOutboxRepository.
    pub fn new(pool: SqlitePool) -> Self {
        SyncOutboxRepository { pool }
    }

    /// Queues an entity for synchronization.
    ///
    /// ## Arguments
    /// * `entity_type` - Type of entity: "SALE", "PRODUCT", "PAYMENT", etc.
    /// * `entity_id` - The entity's UUID
    /// * `payload` - JSON serialization of the full entity
    ///
    /// ## Example
    /// ```rust,ignore
    /// let payload = serde_json::to_string(&sale)?;
    /// repo.queue_for_sync("SALE", &sale.id, &payload).await?;
    /// ```
    pub async fn queue_for_sync(
        &self,
        entity_type: &str,
        entity_id: &str,
        payload: &str,
    ) -> DbResult<SyncOutboxEntry> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();

        debug!(
            entity_type = %entity_type,
            entity_id = %entity_id,
            "Queuing for sync"
        );

        let entry = SyncOutboxEntry {
            id: id.clone(),
            tenant_id: DEFAULT_TENANT_ID.to_string(),
            entity_type: entity_type.to_string(),
            entity_id: entity_id.to_string(),
            payload: payload.to_string(),
            attempts: 0,
            last_error: None,
            created_at: now,
            attempted_at: None,
            synced_at: None,
        };

        sqlx::query!(
            r#"
            INSERT INTO sync_outbox (
                id, tenant_id, entity_type, entity_id, payload,
                attempts, last_error, created_at, attempted_at, synced_at
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5,
                ?6, ?7, ?8, ?9, ?10
            )
            "#,
            entry.id,
            entry.tenant_id,
            entry.entity_type,
            entry.entity_id,
            entry.payload,
            entry.attempts,
            entry.last_error,
            entry.created_at,
            entry.attempted_at,
            entry.synced_at
        )
        .execute(&self.pool)
        .await?;

        Ok(entry)
    }

    /// Gets pending entries that need to be synced.
    ///
    /// ## Arguments
    /// * `limit` - Maximum entries to return
    ///
    /// ## Returns
    /// Entries where `synced_at IS NULL`, ordered by created_at (oldest first).
    pub async fn get_pending(&self, limit: u32) -> DbResult<Vec<SyncOutboxEntry>> {
        let entries = sqlx::query_as!(
            SyncOutboxEntry,
            r#"
            SELECT 
                id,
                tenant_id,
                entity_type,
                entity_id,
                payload,
                attempts,
                last_error,
                created_at as "created_at: chrono::DateTime<Utc>",
                attempted_at as "attempted_at: chrono::DateTime<Utc>",
                synced_at as "synced_at: chrono::DateTime<Utc>"
            FROM sync_outbox
            WHERE synced_at IS NULL
            ORDER BY created_at ASC
            LIMIT ?1
            "#,
            limit
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(entries)
    }

    /// Marks an entry as successfully synced.
    ///
    /// ## Arguments
    /// * `id` - The outbox entry ID
    pub async fn mark_synced(&self, id: &str) -> DbResult<()> {
        let now = Utc::now();

        sqlx::query!(
            r#"
            UPDATE sync_outbox SET
                synced_at = ?2,
                attempted_at = ?2
            WHERE id = ?1
            "#,
            id,
            now
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Records a sync failure.
    ///
    /// ## Arguments
    /// * `id` - The outbox entry ID
    /// * `error` - Error message describing the failure
    pub async fn mark_failed(&self, id: &str, error: &str) -> DbResult<()> {
        let now = Utc::now();

        sqlx::query!(
            r#"
            UPDATE sync_outbox SET
                attempts = attempts + 1,
                last_error = ?2,
                attempted_at = ?3
            WHERE id = ?1
            "#,
            id,
            error,
            now
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Counts pending sync entries.
    pub async fn count_pending(&self) -> DbResult<i64> {
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM sync_outbox WHERE synced_at IS NULL")
                .fetch_one(&self.pool)
                .await?;

        Ok(count)
    }

    /// Deletes old synced entries (cleanup).
    ///
    /// ## Arguments
    /// * `days_old` - Delete entries synced more than this many days ago
    ///
    /// ## Returns
    /// Number of deleted entries.
    pub async fn cleanup_old_entries(&self, days_old: u32) -> DbResult<u64> {
        let result = sqlx::query!(
            r#"
            DELETE FROM sync_outbox
            WHERE synced_at IS NOT NULL
            AND synced_at < datetime('now', '-' || ?1 || ' days')
            "#,
            days_old
        )
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected())
    }
}
