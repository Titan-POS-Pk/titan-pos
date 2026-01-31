//! # Seed Data Generator
//!
//! Populates the database with test products for development.
//!
//! ## Usage
//! ```bash
//! # Generate 5,000 products (default)
//! cargo run -p titan-db --bin seed
//!
//! # Generate custom amount
//! cargo run -p titan-db --bin seed -- --count 10000
//!
//! # Specify database path
//! cargo run -p titan-db --bin seed -- --db ./data/titan.db
//! ```
//!
//! ## Generated Products
//! Creates realistic product data across categories:
//! - Beverages (sodas, water, juice)
//! - Snacks (chips, candy, cookies)
//! - Dairy (milk, cheese, yogurt)
//! - Frozen (ice cream, frozen meals)
//! - Grocery (canned goods, pasta, rice)
//!
//! Each product has:
//! - Unique SKU: `{CATEGORY}-{INDEX}`
//! - Realistic name
//! - Random price: $0.99 - $19.99
//! - Random stock: 0 - 100
//! - Random tax rate: 0%, 5%, 8.25%, 10%

use chrono::Utc;
use std::env;
use titan_core::{Product, DEFAULT_TENANT_ID};
use titan_db::{Database, DbConfig};
use uuid::Uuid;

/// Product categories for realistic test data
const CATEGORIES: &[(&str, &[&str])] = &[
    (
        "BEV",
        &[
            "Coca-Cola",
            "Pepsi",
            "Sprite",
            "Fanta",
            "Dr Pepper",
            "Mountain Dew",
            "7-Up",
            "Red Bull",
            "Monster Energy",
            "Gatorade",
            "Dasani Water",
            "Evian Water",
            "Orange Juice",
            "Apple Juice",
            "Grape Juice",
            "Lemonade",
            "Iced Tea",
            "Coffee",
            "Hot Chocolate",
            "Milk",
        ],
    ),
    (
        "SNK",
        &[
            "Lays Classic",
            "Doritos Nacho",
            "Cheetos",
            "Pringles",
            "Ruffles",
            "Tostitos",
            "Fritos",
            "Snickers",
            "M&Ms",
            "Reeses",
            "Kit Kat",
            "Twix",
            "Skittles",
            "Starburst",
            "Gummy Bears",
            "Oreos",
            "Chips Ahoy",
            "Nutter Butter",
            "Goldfish",
            "Pretzels",
        ],
    ),
    (
        "DRY",
        &[
            "Whole Milk",
            "2% Milk",
            "Skim Milk",
            "Almond Milk",
            "Oat Milk",
            "Cheddar Cheese",
            "Mozzarella",
            "Swiss Cheese",
            "Cream Cheese",
            "Butter",
            "Greek Yogurt",
            "Regular Yogurt",
            "Sour Cream",
            "Heavy Cream",
            "Half & Half",
            "Eggs Dozen",
            "Eggs Half Dozen",
            "Cottage Cheese",
            "Parmesan",
            "Feta Cheese",
        ],
    ),
    (
        "FRZ",
        &[
            "Vanilla Ice Cream",
            "Chocolate Ice Cream",
            "Strawberry Ice Cream",
            "Cookie Dough Ice Cream",
            "Mint Chip Ice Cream",
            "Frozen Pizza",
            "Frozen Burrito",
            "Frozen Dinner",
            "Ice Cream Bars",
            "Popsicles",
            "Frozen Vegetables",
            "Frozen Fruit",
            "Frozen Waffles",
            "Fish Sticks",
            "Chicken Nuggets",
            "Frozen Fries",
            "Ice Cream Sandwich",
            "Sorbet",
            "Frozen Breakfast",
            "Frozen Pie",
        ],
    ),
    (
        "GRO",
        &[
            "White Bread",
            "Wheat Bread",
            "Pasta Spaghetti",
            "Pasta Penne",
            "Rice White",
            "Rice Brown",
            "Canned Beans",
            "Canned Corn",
            "Canned Tomatoes",
            "Canned Soup",
            "Cereal Cheerios",
            "Cereal Frosted Flakes",
            "Oatmeal",
            "Peanut Butter",
            "Jelly",
            "Honey",
            "Maple Syrup",
            "Flour",
            "Sugar",
            "Salt",
        ],
    ),
];

/// Size variants for products
const SIZES: &[(&str, i64)] = &[
    ("Small", 0),
    ("Medium", 100),
    ("Large", 200),
    ("XL", 350),
    ("12oz", 0),
    ("16oz", 50),
    ("20oz", 100),
    ("2L", 150),
    ("6-Pack", 300),
    ("12-Pack", 500),
];

/// Tax rates in basis points
const TAX_RATES: &[u32] = &[0, 500, 825, 1000];

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse command line arguments
    let args: Vec<String> = env::args().collect();

    let mut count: usize = 5000;
    let mut db_path = String::from("./titan_dev.db");

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--count" | "-c" => {
                if i + 1 < args.len() {
                    count = args[i + 1].parse().unwrap_or(5000);
                    i += 1;
                }
            }
            "--db" | "-d" => {
                if i + 1 < args.len() {
                    db_path = args[i + 1].clone();
                    i += 1;
                }
            }
            "--help" | "-h" => {
                println!("Titan POS Seed Data Generator");
                println!();
                println!("Usage: seed [OPTIONS]");
                println!();
                println!("Options:");
                println!("  -c, --count <N>    Number of products to generate (default: 5000)");
                println!("  -d, --db <PATH>    Database file path (default: ./titan_dev.db)");
                println!("  -h, --help         Show this help message");
                return Ok(());
            }
            _ => {}
        }
        i += 1;
    }

    println!("🌱 Titan POS Seed Data Generator");
    println!("================================");
    println!("Database: {}", db_path);
    println!("Products: {}", count);
    println!();

    // Connect to database
    let config = DbConfig::new(&db_path);
    let db = Database::new(config).await?;

    println!("✓ Connected to database");
    println!("✓ Migrations applied");

    // Check existing products
    let existing = db.products().count().await?;
    if existing > 0 {
        println!("⚠ Database already has {} products", existing);
        println!("  Skipping seed to avoid duplicates.");
        println!("  Delete the database file to regenerate.");
        return Ok(());
    }

    // Generate products
    println!();
    println!("Generating products...");

    let mut generated = 0;
    let start = std::time::Instant::now();

    for (category_idx, (category_code, products)) in CATEGORIES.iter().enumerate() {
        for (product_idx, product_name) in products.iter().enumerate() {
            for (size_idx, (size_name, price_addon)) in SIZES.iter().enumerate() {
                if generated >= count {
                    break;
                }

                let product = generate_product(
                    category_code,
                    product_name,
                    size_name,
                    *price_addon,
                    category_idx * 1000 + product_idx * 20 + size_idx,
                );

                if let Err(e) = db.products().insert(&product).await {
                    eprintln!("Failed to insert {}: {}", product.sku, e);
                    continue;
                }

                generated += 1;

                if generated % 500 == 0 {
                    println!("  Generated {} products...", generated);
                }
            }

            if generated >= count {
                break;
            }
        }

        if generated >= count {
            break;
        }
    }

    let elapsed = start.elapsed();
    println!();
    println!("✓ Generated {} products in {:?}", generated, elapsed);
    println!(
        "  Rate: {:.0} products/second",
        generated as f64 / elapsed.as_secs_f64()
    );

    // Verify FTS
    println!();
    println!("Verifying FTS index...");
    let search_results = db.products().search("cola", 10).await?;
    println!("  Search 'cola': {} results", search_results.len());

    let search_results = db.products().search("BEV", 10).await?;
    println!("  Search 'BEV': {} results", search_results.len());

    println!();
    println!("✓ Seed complete!");

    Ok(())
}

/// Generates a single product with realistic data.
fn generate_product(
    category: &str,
    name: &str,
    size: &str,
    price_addon: i64,
    seed: usize,
) -> Product {
    let now = Utc::now();

    // Generate unique SKU
    let sku = format!("{}-{}-{:03}", category, &name.replace(' ', "")[..3].to_uppercase(), seed);

    // Generate barcode (EAN-13 format, but not valid checksum)
    let barcode = Some(format!("590{:010}", seed));

    // Generate price: base $1.99-$9.99 + size addon
    let base_price = 199 + ((seed * 17) % 800) as i64; // $1.99 - $9.99
    let price_cents = base_price + price_addon;

    // Generate cost (60-80% of price)
    let cost_pct = 60 + (seed % 20) as i64;
    let cost_cents = Some(price_cents * cost_pct / 100);

    // Random tax rate
    let tax_rate_bps = TAX_RATES[seed % TAX_RATES.len()];

    // Random stock (0-100)
    let current_stock = Some((seed % 101) as i64);

    // Full product name with size
    let full_name = format!("{} {}", name, size);

    Product {
        id: Uuid::new_v4().to_string(),
        tenant_id: DEFAULT_TENANT_ID.to_string(),
        sku,
        barcode,
        name: full_name,
        description: None,
        price_cents,
        cost_cents,
        tax_rate_bps,
        track_inventory: true,
        allow_negative_stock: false,
        current_stock,
        is_active: true,
        created_at: now,
        updated_at: now,
        sync_version: 0,
    }
}
