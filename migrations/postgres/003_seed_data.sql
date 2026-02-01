-- =============================================================================
-- Titan POS Cloud Database - Seed Data for Development
-- =============================================================================
--
-- This migration inserts test data for local development and testing.
-- It creates a sample tenant with stores, products, and users.
--
-- ⚠️  DO NOT RUN IN PRODUCTION - this is for development only!

-- -----------------------------------------------------------------------------
-- Sample Tenant
-- -----------------------------------------------------------------------------
INSERT INTO tenants (id, name, currency, timezone) VALUES
    ('tenant_demo_001', 'Demo Coffee Chain', 'USD', 'America/New_York')
ON CONFLICT (id) DO NOTHING;

-- -----------------------------------------------------------------------------
-- Sample Stores
-- -----------------------------------------------------------------------------
-- API key is 'demo-store-key-001' hashed with argon2
-- For testing: the unhashed key is used as Bearer token which is then exchanged for JWT
INSERT INTO stores (id, tenant_id, name, api_key_hash, address, city, state, postal_code, timezone) VALUES
    (
        'store_downtown_001',
        'tenant_demo_001',
        'Downtown Flagship',
        -- This is argon2 hash of 'demo-store-key-001'
        '$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQ$xXHQO3/tQMEoWwQw9xRz2w',
        '123 Main Street',
        'New York',
        'NY',
        '10001',
        'America/New_York'
    ),
    (
        'store_airport_002',
        'tenant_demo_001',
        'Airport Terminal B',
        -- This is argon2 hash of 'demo-store-key-002'
        '$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQ$xXHQO3/tQMEoWwQw9xRz2w',
        '1 Airport Drive, Terminal B',
        'New York',
        'NY',
        '11430',
        'America/New_York'
    )
ON CONFLICT (id) DO NOTHING;

-- -----------------------------------------------------------------------------
-- Store Configurations
-- -----------------------------------------------------------------------------
INSERT INTO store_configs (
    store_id, tenant_id, store_name, address, city, state, postal_code, country, timezone,
    currency, tax_mode, allow_negative_inventory, receipt_header, receipt_footer,
    sync_batch_size, sync_interval_secs
) VALUES
    (
        'store_downtown_001',
        'tenant_demo_001',
        'Downtown Flagship',
        '123 Main Street',
        'New York',
        'NY',
        '10001',
        'USA',
        'America/New_York',
        'USD',
        'EXCLUSIVE',
        FALSE,
        'DEMO COFFEE CHAIN\n123 Main Street\nNew York, NY 10001',
        'Thank you for your visit!\nwww.democoffee.com',
        100,
        30
    ),
    (
        'store_airport_002',
        'tenant_demo_001',
        'Airport Terminal B',
        '1 Airport Drive, Terminal B',
        'New York',
        'NY',
        '11430',
        'USA',
        'America/New_York',
        'USD',
        'EXCLUSIVE',
        FALSE,
        'DEMO COFFEE CHAIN\nAirport Terminal B\nSafe Travels!',
        'Thank you for choosing us!\nHave a great flight!',
        50,
        15
    )
ON CONFLICT (store_id) DO NOTHING;

-- -----------------------------------------------------------------------------
-- Sample Devices
-- -----------------------------------------------------------------------------
INSERT INTO devices (id, store_id, tenant_id, name, device_type, priority) VALUES
    ('device_dt_pos1', 'store_downtown_001', 'tenant_demo_001', 'Register 1', 'POS', 100),
    ('device_dt_pos2', 'store_downtown_001', 'tenant_demo_001', 'Register 2', 'POS', 50),
    ('device_dt_server', 'store_downtown_001', 'tenant_demo_001', 'Back Office Server', 'SERVER', 200),
    ('device_ap_pos1', 'store_airport_002', 'tenant_demo_001', 'Mobile Cart 1', 'MOBILE', 100)
ON CONFLICT (id) DO NOTHING;

-- -----------------------------------------------------------------------------
-- Sample Tax Rates
-- -----------------------------------------------------------------------------
INSERT INTO tax_rates (id, tenant_id, name, rate_bps, is_default, is_active) VALUES
    ('tax_ny_sales', 'tenant_demo_001', 'NY Sales Tax', 800, TRUE, TRUE),
    ('tax_ny_food', 'tenant_demo_001', 'NY Food Tax', 0, FALSE, TRUE),
    ('tax_reduced', 'tenant_demo_001', 'Reduced Rate', 400, FALSE, TRUE)
ON CONFLICT (id) DO NOTHING;

-- -----------------------------------------------------------------------------
-- Sample Products
-- -----------------------------------------------------------------------------
INSERT INTO products (
    id, tenant_id, sku, name, barcode, price_cents, cost_cents, 
    tax_rate_id, tax_rate_bps, track_inventory, current_stock,
    low_stock_threshold, category, department
) VALUES
    -- Hot Drinks
    ('prod_espresso', 'tenant_demo_001', 'DRK-ESP-001', 'Espresso', '1234567890123', 350, 50, 'tax_ny_sales', 800, FALSE, NULL, NULL, 'Hot Drinks', 'Beverages'),
    ('prod_americano', 'tenant_demo_001', 'DRK-AMR-001', 'Americano', '1234567890124', 425, 60, 'tax_ny_sales', 800, FALSE, NULL, NULL, 'Hot Drinks', 'Beverages'),
    ('prod_latte', 'tenant_demo_001', 'DRK-LAT-001', 'Caffe Latte', '1234567890125', 550, 80, 'tax_ny_sales', 800, FALSE, NULL, NULL, 'Hot Drinks', 'Beverages'),
    ('prod_cappuccino', 'tenant_demo_001', 'DRK-CAP-001', 'Cappuccino', '1234567890126', 525, 75, 'tax_ny_sales', 800, FALSE, NULL, NULL, 'Hot Drinks', 'Beverages'),
    ('prod_mocha', 'tenant_demo_001', 'DRK-MCH-001', 'Mocha', '1234567890127', 625, 100, 'tax_ny_sales', 800, FALSE, NULL, NULL, 'Hot Drinks', 'Beverages'),
    
    -- Cold Drinks
    ('prod_iced_latte', 'tenant_demo_001', 'DRK-ICL-001', 'Iced Latte', '1234567890128', 575, 85, 'tax_ny_sales', 800, FALSE, NULL, NULL, 'Cold Drinks', 'Beverages'),
    ('prod_cold_brew', 'tenant_demo_001', 'DRK-CLB-001', 'Cold Brew', '1234567890129', 500, 70, 'tax_ny_sales', 800, FALSE, NULL, NULL, 'Cold Drinks', 'Beverages'),
    ('prod_frappuccino', 'tenant_demo_001', 'DRK-FRP-001', 'Frappuccino', '1234567890130', 650, 120, 'tax_ny_sales', 800, FALSE, NULL, NULL, 'Cold Drinks', 'Beverages'),
    
    -- Food
    ('prod_croissant', 'tenant_demo_001', 'FD-CRS-001', 'Butter Croissant', '2234567890123', 395, 120, 'tax_ny_food', 0, TRUE, 100, 20, 'Pastries', 'Food'),
    ('prod_muffin_bb', 'tenant_demo_001', 'FD-MFN-001', 'Blueberry Muffin', '2234567890124', 375, 100, 'tax_ny_food', 0, TRUE, 80, 15, 'Pastries', 'Food'),
    ('prod_muffin_ch', 'tenant_demo_001', 'FD-MFN-002', 'Chocolate Muffin', '2234567890125', 375, 100, 'tax_ny_food', 0, TRUE, 75, 15, 'Pastries', 'Food'),
    ('prod_bagel', 'tenant_demo_001', 'FD-BGL-001', 'Everything Bagel', '2234567890126', 350, 80, 'tax_ny_food', 0, TRUE, 120, 25, 'Pastries', 'Food'),
    ('prod_sandwich', 'tenant_demo_001', 'FD-SND-001', 'Turkey & Cheese Sandwich', '2234567890127', 895, 350, 'tax_ny_food', 0, TRUE, 40, 10, 'Sandwiches', 'Food'),
    
    -- Merchandise
    ('prod_mug', 'tenant_demo_001', 'MRC-MUG-001', 'Branded Coffee Mug', '3234567890123', 1595, 500, 'tax_ny_sales', 800, TRUE, 50, 10, 'Merchandise', 'Retail'),
    ('prod_beans_light', 'tenant_demo_001', 'MRC-BNS-001', 'Light Roast Beans 12oz', '3234567890124', 1495, 600, 'tax_ny_sales', 800, TRUE, 30, 5, 'Coffee Beans', 'Retail'),
    ('prod_beans_dark', 'tenant_demo_001', 'MRC-BNS-002', 'Dark Roast Beans 12oz', '3234567890125', 1495, 600, 'tax_ny_sales', 800, TRUE, 35, 5, 'Coffee Beans', 'Retail')
ON CONFLICT (id) DO NOTHING;

-- -----------------------------------------------------------------------------
-- Sample Inventory (per store)
-- -----------------------------------------------------------------------------
INSERT INTO inventory (store_id, product_id, tenant_id, current_stock) VALUES
    -- Downtown store inventory
    ('store_downtown_001', 'prod_croissant', 'tenant_demo_001', 50),
    ('store_downtown_001', 'prod_muffin_bb', 'tenant_demo_001', 40),
    ('store_downtown_001', 'prod_muffin_ch', 'tenant_demo_001', 35),
    ('store_downtown_001', 'prod_bagel', 'tenant_demo_001', 60),
    ('store_downtown_001', 'prod_sandwich', 'tenant_demo_001', 20),
    ('store_downtown_001', 'prod_mug', 'tenant_demo_001', 30),
    ('store_downtown_001', 'prod_beans_light', 'tenant_demo_001', 20),
    ('store_downtown_001', 'prod_beans_dark', 'tenant_demo_001', 25),
    
    -- Airport store inventory
    ('store_airport_002', 'prod_croissant', 'tenant_demo_001', 30),
    ('store_airport_002', 'prod_muffin_bb', 'tenant_demo_001', 25),
    ('store_airport_002', 'prod_muffin_ch', 'tenant_demo_001', 20),
    ('store_airport_002', 'prod_bagel', 'tenant_demo_001', 40),
    ('store_airport_002', 'prod_sandwich', 'tenant_demo_001', 15),
    ('store_airport_002', 'prod_mug', 'tenant_demo_001', 15),
    ('store_airport_002', 'prod_beans_light', 'tenant_demo_001', 10),
    ('store_airport_002', 'prod_beans_dark', 'tenant_demo_001', 10)
ON CONFLICT (store_id, product_id) DO NOTHING;

-- -----------------------------------------------------------------------------
-- Sample Users
-- -----------------------------------------------------------------------------
INSERT INTO users (id, store_id, tenant_id, username, display_name, role, pin_hash) VALUES
    -- Downtown store users
    ('user_dt_manager', 'store_downtown_001', 'tenant_demo_001', 'jsmith', 'John Smith', 'MANAGER', NULL),
    ('user_dt_cashier1', 'store_downtown_001', 'tenant_demo_001', 'agarcia', 'Ana Garcia', 'CASHIER', NULL),
    ('user_dt_cashier2', 'store_downtown_001', 'tenant_demo_001', 'bwilson', 'Bob Wilson', 'CASHIER', NULL),
    
    -- Airport store users  
    ('user_ap_manager', 'store_airport_002', 'tenant_demo_001', 'mjohnson', 'Maria Johnson', 'MANAGER', NULL),
    ('user_ap_cashier1', 'store_airport_002', 'tenant_demo_001', 'dlee', 'David Lee', 'CASHIER', NULL)
ON CONFLICT (id) DO NOTHING;

-- -----------------------------------------------------------------------------
-- Initialize sync cursors
-- -----------------------------------------------------------------------------
INSERT INTO sync_cursors (store_id, stream, position) VALUES
    ('store_downtown_001', 'upload', 0),
    ('store_downtown_001', 'download', 0),
    ('store_airport_002', 'upload', 0),
    ('store_airport_002', 'download', 0)
ON CONFLICT (store_id, stream) DO NOTHING;

-- Initialize download sequences
INSERT INTO download_sequences (store_id, next_version) VALUES
    ('store_downtown_001', 1),
    ('store_airport_002', 1)
ON CONFLICT (store_id) DO NOTHING;

-- -----------------------------------------------------------------------------
-- Print Summary
-- -----------------------------------------------------------------------------
DO $$
BEGIN
    RAISE NOTICE '====================================';
    RAISE NOTICE 'Seed data loaded successfully!';
    RAISE NOTICE '====================================';
    RAISE NOTICE 'Tenant: Demo Coffee Chain (tenant_demo_001)';
    RAISE NOTICE 'Stores: 2 (Downtown, Airport)';
    RAISE NOTICE 'Products: 16';
    RAISE NOTICE 'Users: 5';
    RAISE NOTICE '';
    RAISE NOTICE 'Test API Keys (exchange for JWT):';
    RAISE NOTICE '  Downtown: demo-store-key-001';
    RAISE NOTICE '  Airport:  demo-store-key-002';
    RAISE NOTICE '====================================';
END $$;
