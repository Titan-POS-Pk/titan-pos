-- =============================================================================
-- Titan POS Cloud Database - Pending Downloads Queue
-- =============================================================================
--
-- This migration adds tables for tracking pending downloads that need to be
-- sent to stores. When the cloud receives updates (e.g., product catalog changes),
-- it queues them for each store to download.
--
-- Architecture:
-- - pending_downloads: Queue of updates waiting to be sent to stores
-- - download_acknowledgments: Track what each store has received

-- -----------------------------------------------------------------------------
-- Pending Downloads - Queue of updates to send to stores
-- -----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS pending_downloads (
    id BIGSERIAL PRIMARY KEY,
    tenant_id TEXT NOT NULL REFERENCES tenants(id),
    store_id TEXT NOT NULL REFERENCES stores(id),
    
    -- Entity information
    entity_type TEXT NOT NULL, -- PRODUCT, TAX_RATE, USER, CONFIG, etc.
    entity_id TEXT NOT NULL,
    
    -- Operation type
    operation TEXT NOT NULL, -- INSERT, UPDATE, DELETE
    
    -- Full entity payload as JSON (for convenience)
    payload JSONB NOT NULL,
    
    -- Ordering and versioning
    version BIGINT NOT NULL, -- Sequential per store
    
    -- Status
    status TEXT NOT NULL DEFAULT 'PENDING', -- PENDING, DELIVERED, ACKNOWLEDGED
    
    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    delivered_at TIMESTAMPTZ,
    acknowledged_at TIMESTAMPTZ
);

-- Indexes for efficient querying
CREATE INDEX IF NOT EXISTS idx_pending_downloads_store ON pending_downloads(store_id);
CREATE INDEX IF NOT EXISTS idx_pending_downloads_tenant ON pending_downloads(tenant_id);
CREATE INDEX IF NOT EXISTS idx_pending_downloads_pending ON pending_downloads(store_id, status, version) 
    WHERE status = 'PENDING';
CREATE INDEX IF NOT EXISTS idx_pending_downloads_entity ON pending_downloads(entity_type, entity_id);

-- -----------------------------------------------------------------------------
-- Download Sequence - Track next version number per store
-- -----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS download_sequences (
    store_id TEXT PRIMARY KEY REFERENCES stores(id),
    next_version BIGINT NOT NULL DEFAULT 1,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Trigger to update updated_at
CREATE TRIGGER update_download_sequences_updated_at 
    BEFORE UPDATE ON download_sequences 
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

-- -----------------------------------------------------------------------------
-- Function: Queue a download for a specific store
-- -----------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION queue_download_for_store(
    p_tenant_id TEXT,
    p_store_id TEXT,
    p_entity_type TEXT,
    p_entity_id TEXT,
    p_operation TEXT,
    p_payload JSONB
) RETURNS BIGINT AS $$
DECLARE
    v_version BIGINT;
BEGIN
    -- Get and increment the version
    INSERT INTO download_sequences (store_id, next_version)
    VALUES (p_store_id, 2)
    ON CONFLICT (store_id) DO UPDATE SET
        next_version = download_sequences.next_version + 1,
        updated_at = NOW()
    RETURNING next_version - 1 INTO v_version;
    
    -- Insert the download record
    INSERT INTO pending_downloads (
        tenant_id, store_id, entity_type, entity_id, operation, payload, version
    ) VALUES (
        p_tenant_id, p_store_id, p_entity_type, p_entity_id, p_operation, p_payload, v_version
    );
    
    RETURN v_version;
END;
$$ LANGUAGE plpgsql;

-- -----------------------------------------------------------------------------
-- Function: Queue a download for all stores in a tenant
-- -----------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION queue_download_for_tenant(
    p_tenant_id TEXT,
    p_entity_type TEXT,
    p_entity_id TEXT,
    p_operation TEXT,
    p_payload JSONB
) RETURNS VOID AS $$
DECLARE
    v_store_id TEXT;
BEGIN
    FOR v_store_id IN SELECT id FROM stores WHERE tenant_id = p_tenant_id AND is_active = TRUE
    LOOP
        PERFORM queue_download_for_store(
            p_tenant_id, v_store_id, p_entity_type, p_entity_id, p_operation, p_payload
        );
    END LOOP;
END;
$$ LANGUAGE plpgsql;

-- -----------------------------------------------------------------------------
-- Trigger: Auto-queue product updates to all tenant stores
-- -----------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION queue_product_download()
RETURNS TRIGGER AS $$
DECLARE
    v_operation TEXT;
    v_payload JSONB;
BEGIN
    -- Determine operation
    IF TG_OP = 'INSERT' THEN
        v_operation := 'INSERT';
        v_payload := row_to_json(NEW)::JSONB;
    ELSIF TG_OP = 'UPDATE' THEN
        v_operation := 'UPDATE';
        v_payload := row_to_json(NEW)::JSONB;
    ELSIF TG_OP = 'DELETE' THEN
        v_operation := 'DELETE';
        v_payload := row_to_json(OLD)::JSONB;
        -- Queue delete for all stores
        PERFORM queue_download_for_tenant(
            OLD.tenant_id, 'PRODUCT', OLD.id, v_operation, v_payload
        );
        RETURN OLD;
    END IF;
    
    -- Queue for all stores
    PERFORM queue_download_for_tenant(
        NEW.tenant_id, 'PRODUCT', NEW.id, v_operation, v_payload
    );
    
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER auto_queue_product_downloads
    AFTER INSERT OR UPDATE OR DELETE ON products
    FOR EACH ROW EXECUTE FUNCTION queue_product_download();

-- -----------------------------------------------------------------------------
-- Trigger: Auto-queue tax rate updates to all tenant stores
-- -----------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION queue_tax_rate_download()
RETURNS TRIGGER AS $$
DECLARE
    v_operation TEXT;
    v_payload JSONB;
BEGIN
    IF TG_OP = 'INSERT' THEN
        v_operation := 'INSERT';
        v_payload := row_to_json(NEW)::JSONB;
    ELSIF TG_OP = 'UPDATE' THEN
        v_operation := 'UPDATE';
        v_payload := row_to_json(NEW)::JSONB;
    ELSIF TG_OP = 'DELETE' THEN
        v_operation := 'DELETE';
        v_payload := row_to_json(OLD)::JSONB;
        PERFORM queue_download_for_tenant(
            OLD.tenant_id, 'TAX_RATE', OLD.id, v_operation, v_payload
        );
        RETURN OLD;
    END IF;
    
    PERFORM queue_download_for_tenant(
        NEW.tenant_id, 'TAX_RATE', NEW.id, v_operation, v_payload
    );
    
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER auto_queue_tax_rate_downloads
    AFTER INSERT OR UPDATE OR DELETE ON tax_rates
    FOR EACH ROW EXECUTE FUNCTION queue_tax_rate_download();

-- -----------------------------------------------------------------------------
-- View: Store sync status
-- -----------------------------------------------------------------------------
CREATE OR REPLACE VIEW store_sync_status AS
SELECT 
    s.id AS store_id,
    s.tenant_id,
    s.name AS store_name,
    COALESCE(sc.position, 0) AS upload_cursor,
    COALESCE(ds.next_version - 1, 0) AS download_max_version,
    COALESCE(
        (SELECT COUNT(*) FROM pending_downloads pd 
         WHERE pd.store_id = s.id AND pd.status = 'PENDING'),
        0
    ) AS pending_download_count,
    sc.updated_at AS last_upload_at
FROM stores s
LEFT JOIN sync_cursors sc ON sc.store_id = s.id AND sc.stream = 'upload'
LEFT JOIN download_sequences ds ON ds.store_id = s.id;
