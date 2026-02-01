//! # Configuration State
//!
//! Stores application configuration loaded at startup.
//!
//! ## Configuration Sources (Priority Order)
//! 1. Environment variables (`TITAN_*`)
//! 2. Config file (`config.toml`)
//! 3. Database (`config` table)
//! 4. Defaults (this file)
//!
//! ## Thread Safety
//! Configuration is read-only after initialization, so no mutex needed.
//! If hot-reloading is added later, we'd wrap in `RwLock`.

use serde::{Deserialize, Serialize};
use titan_core::DEFAULT_TENANT_ID;

/// Application configuration.
///
/// ## Fields
/// Most fields have sensible defaults for development.
/// Production deployments should configure these properly.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigState {
    /// Tenant ID for multi-tenant support.
    /// Default: "default" (single-tenant mode)
    pub tenant_id: String,

    /// Store name (displayed on receipts)
    pub store_name: String,

    /// Store address lines (for receipts)
    pub store_address: Vec<String>,

    /// Currency code (ISO 4217)
    pub currency_code: String,

    /// Currency symbol (for display)
    pub currency_symbol: String,

    /// Number of decimal places for currency
    pub currency_decimals: u8,

    /// Default tax rate in basis points
    /// e.g., 825 = 8.25%
    pub default_tax_rate_bps: u32,

    /// Tax calculation mode
    pub tax_mode: TaxMode,

    /// Enable sound effects
    pub sound_enabled: bool,

    /// Receipt printer configuration
    pub receipt_printer: Option<PrinterConfig>,
}

/// How tax is calculated on items.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TaxMode {
    /// Tax is calculated on top of the price (US style)
    /// Display: $10.00 + $0.83 tax = $10.83
    #[default]
    Exclusive,

    /// Tax is included in the displayed price (EU style)
    /// Display: $10.83 (includes $0.83 tax)
    Inclusive,
}

/// Printer configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrinterConfig {
    /// Printer type
    pub printer_type: PrinterType,

    /// Connection string (e.g., USB path, IP address)
    pub connection: String,

    /// Paper width in characters (typically 32, 42, or 48)
    pub paper_width: u8,
}

/// Supported printer types.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PrinterType {
    /// ESC/POS thermal printer
    EscPos,

    /// Star printer
    Star,

    /// System default printer (for development)
    System,
}

impl Default for ConfigState {
    /// Returns default configuration suitable for development.
    ///
    /// ## Default Values
    /// - Store: "Titan POS Dev Store"
    /// - Currency: USD ($)
    /// - Tax: 8.25% exclusive
    /// - Sounds: enabled
    /// - Printer: none (dev mode)
    fn default() -> Self {
        ConfigState {
            tenant_id: DEFAULT_TENANT_ID.to_string(),
            store_name: "Titan POS Dev Store".to_string(),
            store_address: vec!["123 Main Street".to_string(), "City, ST 12345".to_string()],
            currency_code: "USD".to_string(),
            currency_symbol: "$".to_string(),
            currency_decimals: 2,
            default_tax_rate_bps: 825, // 8.25%
            tax_mode: TaxMode::Exclusive,
            sound_enabled: true,
            receipt_printer: None,
        }
    }
}

impl ConfigState {
    /// Creates a new ConfigState from environment variables and defaults.
    ///
    /// ## Environment Variables
    /// - `TITAN_TENANT_ID`: Override tenant ID
    /// - `TITAN_STORE_NAME`: Override store name
    /// - `TITAN_TAX_RATE`: Override default tax rate (e.g., "8.25")
    pub fn from_env() -> Self {
        let mut config = ConfigState::default();

        if let Ok(tenant_id) = std::env::var("TITAN_TENANT_ID") {
            config.tenant_id = tenant_id;
        }

        if let Ok(store_name) = std::env::var("TITAN_STORE_NAME") {
            config.store_name = store_name;
        }

        if let Ok(tax_rate_str) = std::env::var("TITAN_TAX_RATE") {
            if let Ok(rate) = tax_rate_str.parse::<f64>() {
                config.default_tax_rate_bps = (rate * 100.0) as u32;
            }
        }

        config
    }

    /// Formats a cent amount as a currency string.
    ///
    /// ## Example
    /// ```rust,ignore
    /// let config = ConfigState::default();
    /// assert_eq!(config.format_currency(1234), "$12.34");
    /// ```
    pub fn format_currency(&self, cents: i64) -> String {
        let divisor = 10_i64.pow(self.currency_decimals as u32);
        let whole = cents / divisor;
        let frac = (cents % divisor).abs();

        format!(
            "{}{}{}",
            if cents < 0 { "-" } else { "" },
            self.currency_symbol,
            if self.currency_decimals > 0 {
                format!(
                    "{}.{:0width$}",
                    whole.abs(),
                    frac,
                    width = self.currency_decimals as usize
                )
            } else {
                whole.abs().to_string()
            }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_currency_positive() {
        let config = ConfigState::default();
        assert_eq!(config.format_currency(1234), "$12.34");
        assert_eq!(config.format_currency(100), "$1.00");
        assert_eq!(config.format_currency(1), "$0.01");
        assert_eq!(config.format_currency(0), "$0.00");
    }

    #[test]
    fn test_format_currency_negative() {
        let config = ConfigState::default();
        assert_eq!(config.format_currency(-1234), "-$12.34");
    }

    #[test]
    fn test_format_currency_large() {
        let config = ConfigState::default();
        assert_eq!(config.format_currency(123456789), "$1234567.89");
    }
}
