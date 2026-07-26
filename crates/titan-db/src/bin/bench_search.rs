//! # Search Benchmark
//!
//! Reproduces the search latency table in the README.
//!
//! ## Usage
//! ```bash
//! # Full run: 50,000 products, 500 iterations per query
//! cargo run --release -p titan-db --bin bench_search
//!
//! # Smaller/faster run
//! cargo run --release -p titan-db --bin bench_search -- --count 5000 --iterations 100
//!
//! # Keep the seeded database instead of using a temp file
//! cargo run --release -p titan-db --bin bench_search -- --db ./data/bench.db
//! ```
//!
//! ## Method
//! The database is seeded once, then each query is run `--iterations` times
//! against a **single already-open connection pool**, so process startup,
//! migration and connection setup are outside the measurement. A warmup pass
//! runs before timing so the first-call page cache miss is not counted.
//!
//! Reported figure is the **median**, with p95 and max alongside it, because
//! the distribution has a long tail and a mean would hide it.
//!
//! ## What the numbers mean
//! Cost tracks the number of *matches*, not the size of the catalogue:
//! `ORDER BY rank` scores every hit before `LIMIT` applies. A selective query
//! (a SKU, a barcode, a product name) is what a lane actually types and stays
//! well under 10ms. A deliberately unselective term is not, and is included
//! for exactly that reason.

use std::env;
use std::time::{Duration, Instant};

use chrono::Utc;
use titan_core::{Product, DEFAULT_TENANT_ID};
use titan_db::{Database, DbConfig};
use uuid::Uuid;

/// A query to measure, with a human label for the table.
struct Case {
    label: &'static str,
    query: &'static str,
}

/// The cases published in the README, ordered from what a lane actually does
/// to the pathological case. The first two are the whole job: a cashier types
/// a SKU or a scanner sends a barcode. The last one is in the table because it
/// is where the "sub-10ms" claim stops holding.
const CASES: &[Case] = &[
    Case {
        label: "Full SKU (typed)",
        query: "BEV-COC-000",
    },
    Case {
        label: "Full barcode (scanned)",
        query: "5900000000042",
    },
    Case {
        label: "Full product name",
        query: "Cookie Dough Ice Cream",
    },
    Case {
        label: "Bare category prefix",
        query: "BEV",
    },
];

/// Result limit used for every measured query, matching the UI.
const LIMIT: u32 = 20;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();

    let mut count: usize = 50_000;
    let mut iterations: usize = 500;
    let mut db_path: Option<String> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--count" | "-c" if i + 1 < args.len() => {
                count = args[i + 1].parse()?;
                i += 1;
            }
            "--iterations" | "-n" if i + 1 < args.len() => {
                iterations = args[i + 1].parse()?;
                i += 1;
            }
            "--db" | "-d" if i + 1 < args.len() => {
                db_path = Some(args[i + 1].clone());
                i += 1;
            }
            "--help" | "-h" => {
                println!("Titan POS search benchmark");
                println!();
                println!("Usage: bench_search [OPTIONS]");
                println!();
                println!("Options:");
                println!("  -c, --count <N>       Products to seed (default: 50000)");
                println!("  -n, --iterations <N>  Iterations per query (default: 500)");
                println!("  -d, --db <PATH>       Database path (default: a temp file)");
                println!("  -h, --help            Show this message");
                return Ok(());
            }
            _ => {}
        }
        i += 1;
    }

    // A temp path unless one was given, so a run never clobbers a real
    // database and never leaves one behind.
    let (path, temporary) = match db_path {
        Some(p) => (std::path::PathBuf::from(p), false),
        None => (
            env::temp_dir().join(format!("titan-bench-{}.db", Uuid::new_v4())),
            true,
        ),
    };

    println!("Titan POS search benchmark");
    println!("==========================");
    println!("Database:   {}", path.display());
    println!("Products:   {}", count);
    println!("Iterations: {} per query", iterations);
    println!();

    let db = Database::new(DbConfig::new(&path)).await?;

    let existing = db.products().count().await? as usize;
    if existing < count {
        println!("Seeding {} products...", count - existing);
        let start = Instant::now();
        seed(&db, existing, count).await?;
        println!(
            "Seeded in {:.1}s ({:.0} rows/s)",
            start.elapsed().as_secs_f64(),
            (count - existing) as f64 / start.elapsed().as_secs_f64()
        );
    }

    let total = db.products().count().await?;
    println!("Catalogue:  {} products", total);
    println!();

    // Warmup, outside the measurement: the first query of a run pays for cold
    // page cache and for sqlx preparing the statement.
    for case in CASES {
        let _ = db.products().search(case.query, LIMIT).await?;
    }

    println!(
        "| {:<24} | {:>13} | {:>9} | {:>9} | {:>9} |",
        "Query", "Rows matched", "Median", "p95", "Max"
    );
    println!(
        "|{:-<26}|{:->15}|{:->11}|{:->11}|{:->11}|",
        "", "", "", "", ""
    );

    for case in CASES {
        // Rows the query matches in total, which is the number that explains
        // the latency. Deliberately not the LIMIT-ed count.
        let matched = db.products().search(case.query, u32::MAX).await?.len();

        let mut samples: Vec<Duration> = Vec::with_capacity(iterations);
        for _ in 0..iterations {
            let start = Instant::now();
            let _ = db.products().search(case.query, LIMIT).await?;
            samples.push(start.elapsed());
        }
        samples.sort_unstable();

        let median = samples[samples.len() / 2];
        let p95 = samples[(samples.len() * 95) / 100];
        let max = samples[samples.len() - 1];

        println!(
            "| {:<24} | {:>13} | {:>7.2}ms | {:>7.2}ms | {:>7.2}ms |",
            case.label,
            matched,
            median.as_secs_f64() * 1000.0,
            p95.as_secs_f64() * 1000.0,
            max.as_secs_f64() * 1000.0,
        );
    }

    println!();
    println!("Cost tracks match count, not catalogue size: ORDER BY rank scores");
    println!("every hit before LIMIT applies.");

    if temporary {
        drop(db);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("db-wal"));
        let _ = std::fs::remove_file(path.with_extension("db-shm"));
    }

    Ok(())
}

/// Product categories, mirroring `seed.rs` so the two produce the same shape
/// of catalogue.
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

/// Size variants, mirroring `seed.rs`.
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

const TAX_RATES: &[u32] = &[0, 500, 825, 1000];

/// Fills the catalogue up to `target`, cycling the fixed name list and
/// suffixing each pass so SKUs, barcodes and names stay unique.
async fn seed(db: &Database, from: usize, target: usize) -> Result<(), Box<dyn std::error::Error>> {
    let mut generated = from;
    let mut inserted_since_log = 0usize;

    'outer: for pass in 0.. {
        for (category, names) in CATEGORIES.iter() {
            for name in names.iter() {
                for (size, price_addon) in SIZES.iter() {
                    if generated >= target {
                        break 'outer;
                    }

                    let product = make_product(category, name, size, *price_addon, generated, pass);
                    db.products().insert(&product).await?;

                    generated += 1;
                    inserted_since_log += 1;
                    if inserted_since_log >= 10_000 {
                        println!("  {} / {}", generated, target);
                        inserted_since_log = 0;
                    }
                }
            }
        }
    }

    Ok(())
}

fn make_product(
    category: &str,
    name: &str,
    size: &str,
    price_addon: i64,
    seed: usize,
    pass: usize,
) -> Product {
    let now = Utc::now();

    let sku = format!(
        "{}-{}-{:03}",
        category,
        &name.replace(' ', "")[..3].to_uppercase(),
        seed
    );

    let base_price = 199 + ((seed * 17) % 800) as i64;
    let price_cents = base_price + price_addon;
    let cost_pct = 60 + (seed % 20) as i64;

    let full_name = if pass == 0 {
        format!("{} {}", name, size)
    } else {
        format!("{} {} v{}", name, size, pass + 1)
    };

    Product {
        id: Uuid::new_v4().to_string(),
        tenant_id: DEFAULT_TENANT_ID.to_string(),
        sku,
        barcode: Some(format!("590{:010}", seed)),
        name: full_name,
        description: None,
        price_cents,
        cost_cents: Some(price_cents * cost_pct / 100),
        tax_rate_bps: TAX_RATES[seed % TAX_RATES.len()],
        track_inventory: true,
        allow_negative_stock: false,
        current_stock: Some((seed % 101) as i64),
        is_active: true,
        created_at: now,
        updated_at: now,
        sync_version: 0,
    }
}
