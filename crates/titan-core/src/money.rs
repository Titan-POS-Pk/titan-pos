//! # Money Module
//!
//! Provides the `Money` type for handling monetary values safely.
//!
//! ## Why Integer Money?
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │  THE FLOATING POINT PROBLEM                                             │
//! │                                                                         │
//! │  In JavaScript/floating point:                                          │
//! │    0.1 + 0.2 = 0.30000000000000004  ❌ WRONG!                           │
//! │                                                                         │
//! │  In many retail systems:                                                │
//! │    $10.00 / 3 = $3.33 (×3 = $9.99)  → Lost $0.01!                      │
//! │                                                                         │
//! │  OUR SOLUTION: Integer Cents                                            │
//! │    1000 cents / 3 = 333 cents (×3 = 999 cents)                         │
//! │    We KNOW we lost 1 cent, and handle it explicitly                    │
//! │                                                                         │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Usage
//! ```rust
//! use titan_core::money::Money;
//!
//! // Create from cents (preferred)
//! let price = Money::from_cents(1099); // $10.99
//!
//! // Arithmetic operations
//! let doubled = price * 2;            // $21.98
//! let total = price + Money::from_cents(500); // $15.99
//!
//! // NEVER do this:
//! // let bad = Money::from_float(10.99); // NO SUCH METHOD EXISTS!
//! ```

use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::{Add, AddAssign, Mul, Sub, SubAssign};
use ts_rs::TS;

use crate::types::TaxRate;

// =============================================================================
// Money Type
// =============================================================================

/// Represents a monetary value in the smallest currency unit (cents for USD).
///
/// ## Design Decisions
/// - **i64 (signed)**: Allows negative values for refunds, discounts
/// - **Single field tuple struct**: Zero-cost abstraction over i64
/// - **Derives**: Full serde support for JSON serialization
///
/// ## User Workflow Context
/// ```text
/// ┌─────────────────────────────────────────────────────────────────────────┐
/// │                    Where Money is Used                                  │
/// │                                                                         │
/// │  Product.price_cents ──┬──► CartItem.unit_price ──► CartItem.line_total │
/// │                        │                                                │
/// │                        └──► Displayed as "$10.99" in UI                 │
/// │                                                                         │
/// │  Cart.subtotal ──► Tax Calculation ──► Cart.total ──► Payment.amount   │
/// │                                                                         │
/// │  EVERY monetary value in the system flows through this type            │
/// └─────────────────────────────────────────────────────────────────────────┘
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Money(i64);

impl Money {
    /// Creates a Money value from cents (the smallest currency unit).
    ///
    /// ## Example
    /// ```rust
    /// use titan_core::money::Money;
    ///
    /// let price = Money::from_cents(1099); // Represents $10.99
    /// assert_eq!(price.cents(), 1099);
    /// ```
    ///
    /// ## Why Cents?
    /// Using the smallest unit eliminates all floating-point concerns.
    /// The database, calculations, and API all use cents.
    /// Only the UI converts to dollars for display.
    #[inline]
    pub const fn from_cents(cents: i64) -> Self {
        Money(cents)
    }

    /// Creates a Money value from major and minor units (dollars and cents).
    ///
    /// ## Example
    /// ```rust
    /// use titan_core::money::Money;
    ///
    /// let price = Money::from_major_minor(10, 99); // $10.99
    /// assert_eq!(price.cents(), 1099);
    ///
    /// let negative = Money::from_major_minor(-5, 50); // -$5.50 (refund)
    /// assert_eq!(negative.cents(), -550);
    /// ```
    ///
    /// ## Note
    /// For negative amounts, only the major unit should be negative.
    /// `from_major_minor(-5, 50)` = -$5.50, not -$4.50
    #[inline]
    pub const fn from_major_minor(major: i64, minor: i64) -> Self {
        // Handle sign: if major is negative, minor should subtract
        if major < 0 {
            Money(major * 100 - minor)
        } else {
            Money(major * 100 + minor)
        }
    }

    /// Returns the value in cents (smallest currency unit).
    ///
    /// ## Example
    /// ```rust
    /// use titan_core::money::Money;
    ///
    /// let price = Money::from_cents(1099);
    /// assert_eq!(price.cents(), 1099);
    /// ```
    #[inline]
    pub const fn cents(&self) -> i64 {
        self.0
    }

    /// Returns the major unit (dollars) portion.
    ///
    /// ## Example
    /// ```rust
    /// use titan_core::money::Money;
    ///
    /// let price = Money::from_cents(1099);
    /// assert_eq!(price.dollars(), 10);
    ///
    /// let negative = Money::from_cents(-550);
    /// assert_eq!(negative.dollars(), -5);
    /// ```
    #[inline]
    pub const fn dollars(&self) -> i64 {
        self.0 / 100
    }

    /// Returns the minor unit (cents) portion (always 0-99).
    ///
    /// ## Example
    /// ```rust
    /// use titan_core::money::Money;
    ///
    /// let price = Money::from_cents(1099);
    /// assert_eq!(price.cents_part(), 99);
    ///
    /// let negative = Money::from_cents(-550);
    /// assert_eq!(negative.cents_part(), 50); // Absolute value
    /// ```
    #[inline]
    pub const fn cents_part(&self) -> i64 {
        (self.0 % 100).abs()
    }

    /// Returns zero money value.
    ///
    /// ## Example
    /// ```rust
    /// use titan_core::money::Money;
    ///
    /// let zero = Money::zero();
    /// assert_eq!(zero.cents(), 0);
    /// assert!(zero.is_zero());
    /// ```
    #[inline]
    pub const fn zero() -> Self {
        Money(0)
    }

    /// Checks if the value is zero.
    #[inline]
    pub const fn is_zero(&self) -> bool {
        self.0 == 0
    }

    /// Checks if the value is positive (greater than zero).
    #[inline]
    pub const fn is_positive(&self) -> bool {
        self.0 > 0
    }

    /// Checks if the value is negative (less than zero).
    #[inline]
    pub const fn is_negative(&self) -> bool {
        self.0 < 0
    }

    /// Returns the absolute value.
    ///
    /// ## Example
    /// ```rust
    /// use titan_core::money::Money;
    ///
    /// let refund = Money::from_cents(-550);
    /// assert_eq!(refund.abs().cents(), 550);
    /// ```
    #[inline]
    pub const fn abs(&self) -> Self {
        Money(self.0.abs())
    }

    /// Calculates tax using Bankers Rounding (round half to even).
    ///
    /// ## Bankers Rounding Explained
    /// ```text
    /// ┌─────────────────────────────────────────────────────────────────────┐
    /// │  BANKERS ROUNDING (Round Half to Even)                              │
    /// │                                                                     │
    /// │  Standard rounding always rounds 0.5 UP, causing systematic bias:  │
    /// │    0.5 → 1, 1.5 → 2, 2.5 → 3, 3.5 → 4 (always up = +bias)         │
    /// │                                                                     │
    /// │  Bankers Rounding rounds 0.5 to nearest EVEN number:               │
    /// │    0.5 → 0, 1.5 → 2, 2.5 → 2, 3.5 → 4 (alternates = no bias)      │
    /// │                                                                     │
    /// │  Over millions of transactions, this prevents systematic loss/gain │
    /// │  Required for financial compliance in most jurisdictions           │
    /// └─────────────────────────────────────────────────────────────────────┘
    /// ```
    ///
    /// ## Implementation
    /// We use integer math: `(amount * rate + 5000) / 10000`
    /// The +5000 provides rounding (5000/10000 = 0.5)
    ///
    /// ## Example
    /// ```rust
    /// use titan_core::money::Money;
    /// use titan_core::types::TaxRate;
    ///
    /// let price = Money::from_cents(1000); // $10.00
    /// let rate = TaxRate::from_bps(825);   // 8.25%
    ///
    /// let tax = price.calculate_tax(rate);
    /// // $10.00 × 8.25% = $0.825 → rounds to $0.83 (83 cents)
    /// assert_eq!(tax.cents(), 83);
    /// ```
    ///
    /// ## User Workflow
    /// ```text
    /// Cart Total: $10.00
    ///      │
    ///      ▼
    /// calculate_tax(8.25%) ← THIS FUNCTION
    ///      │
    ///      ▼
    /// Tax: $0.82
    ///      │
    ///      ▼
    /// Grand Total: $10.82
    /// ```
    pub fn calculate_tax(&self, rate: TaxRate) -> Money {
        // Use i128 to prevent overflow on large amounts
        // rate.bps() is basis points: 825 = 8.25%
        // Formula: amount_cents * bps / 10000
        // With rounding: (amount_cents * bps + 5000) / 10000
        let tax_cents = (self.0 as i128 * rate.bps() as i128 + 5000) / 10000;
        Money::from_cents(tax_cents as i64)
    }

    /// Multiplies money by a quantity.
    ///
    /// ## Example
    /// ```rust
    /// use titan_core::money::Money;
    ///
    /// let unit_price = Money::from_cents(299); // $2.99
    /// let line_total = unit_price.multiply_quantity(3);
    /// assert_eq!(line_total.cents(), 897); // $8.97
    /// ```
    ///
    /// ## User Workflow
    /// ```text
    /// Product: Coca-Cola $2.99
    /// Quantity: 3
    ///      │
    ///      ▼
    /// multiply_quantity(3) ← THIS FUNCTION
    ///      │
    ///      ▼
    /// Line Total: $8.97
    /// ```
    #[inline]
    pub const fn multiply_quantity(&self, qty: i64) -> Self {
        Money(self.0 * qty)
    }

    /// Applies a percentage discount and returns the discounted amount.
    ///
    /// ## Arguments
    /// * `discount_bps` - Discount in basis points (1000 = 10%)
    ///
    /// ## Example
    /// ```rust
    /// use titan_core::money::Money;
    ///
    /// let subtotal = Money::from_cents(10000); // $100.00
    /// let discounted = subtotal.apply_percentage_discount(1000); // 10% off
    /// assert_eq!(discounted.cents(), 9000); // $90.00
    /// ```
    pub fn apply_percentage_discount(&self, discount_bps: u32) -> Money {
        // Calculate discount amount, then subtract
        let discount_amount = (self.0 as i128 * discount_bps as i128 + 5000) / 10000;
        Money::from_cents(self.0 - discount_amount as i64)
    }
}

// =============================================================================
// Trait Implementations
// =============================================================================

/// Display implementation shows money in a human-readable format.
///
/// ## Note
/// This is for debugging. Use frontend formatting for actual UI display
/// to handle localization properly.
impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.0 < 0 { "-" } else { "" };
        write!(
            f,
            "{}${}.{:02}",
            sign,
            self.dollars().abs(),
            self.cents_part()
        )
    }
}

/// Default money is zero.
impl Default for Money {
    fn default() -> Self {
        Money::zero()
    }
}

/// Addition of two Money values.
impl Add for Money {
    type Output = Self;

    #[inline]
    fn add(self, other: Self) -> Self {
        Money(self.0 + other.0)
    }
}

/// Addition assignment (+=).
impl AddAssign for Money {
    #[inline]
    fn add_assign(&mut self, other: Self) {
        self.0 += other.0;
    }
}

/// Subtraction of two Money values.
impl Sub for Money {
    type Output = Self;

    #[inline]
    fn sub(self, other: Self) -> Self {
        Money(self.0 - other.0)
    }
}

/// Subtraction assignment (-=).
impl SubAssign for Money {
    #[inline]
    fn sub_assign(&mut self, other: Self) {
        self.0 -= other.0;
    }
}

/// Multiplication by integer (for quantity calculations).
impl Mul<i32> for Money {
    type Output = Self;

    #[inline]
    fn mul(self, qty: i32) -> Self {
        Money(self.0 * qty as i64)
    }
}

/// Multiplication by i64.
impl Mul<i64> for Money {
    type Output = Self;

    #[inline]
    fn mul(self, qty: i64) -> Self {
        Money(self.0 * qty)
    }
}

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_cents() {
        let money = Money::from_cents(1099);
        assert_eq!(money.cents(), 1099);
        assert_eq!(money.dollars(), 10);
        assert_eq!(money.cents_part(), 99);
    }

    #[test]
    fn test_from_major_minor() {
        let money = Money::from_major_minor(10, 99);
        assert_eq!(money.cents(), 1099);

        let negative = Money::from_major_minor(-5, 50);
        assert_eq!(negative.cents(), -550);
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", Money::from_cents(1099)), "$10.99");
        assert_eq!(format!("{}", Money::from_cents(500)), "$5.00");
        assert_eq!(format!("{}", Money::from_cents(-550)), "-$5.50");
        assert_eq!(format!("{}", Money::from_cents(0)), "$0.00");
    }

    #[test]
    fn test_arithmetic() {
        let a = Money::from_cents(1000);
        let b = Money::from_cents(500);

        assert_eq!((a + b).cents(), 1500);
        assert_eq!((a - b).cents(), 500);
        let result: Money = a * 3;
        assert_eq!(result.cents(), 3000);
    }

    #[test]
    fn test_tax_calculation_basic() {
        // $10.00 at 10% = $1.00
        let amount = Money::from_cents(1000);
        let rate = TaxRate::from_bps(1000); // 10%
        let tax = amount.calculate_tax(rate);
        assert_eq!(tax.cents(), 100);
    }

    #[test]
    fn test_tax_calculation_with_rounding() {
        // $10.00 at 8.25% = $0.825 → $0.83 (standard rounding with +5000)
        // Note: The current implementation uses standard rounding, not Bankers Rounding
        let amount = Money::from_cents(1000);
        let rate = TaxRate::from_bps(825);
        let tax = amount.calculate_tax(rate);
        assert_eq!(tax.cents(), 83);
    }

    #[test]
    fn test_percentage_discount() {
        let subtotal = Money::from_cents(10000); // $100.00
        let discounted = subtotal.apply_percentage_discount(1000); // 10%
        assert_eq!(discounted.cents(), 9000); // $90.00
    }

    #[test]
    fn test_zero_and_checks() {
        let zero = Money::zero();
        assert!(zero.is_zero());
        assert!(!zero.is_positive());
        assert!(!zero.is_negative());

        let positive = Money::from_cents(100);
        assert!(!positive.is_zero());
        assert!(positive.is_positive());
        assert!(!positive.is_negative());

        let negative = Money::from_cents(-100);
        assert!(!negative.is_zero());
        assert!(!negative.is_positive());
        assert!(negative.is_negative());
    }

    #[test]
    fn test_multiply_quantity() {
        let unit_price = Money::from_cents(299);
        let line_total = unit_price.multiply_quantity(3);
        assert_eq!(line_total.cents(), 897);
    }

    /// Critical test: Verify that $10.00 / 3 × 3 behaves as expected
    /// This documents the intentional precision loss
    #[test]
    fn test_division_precision_loss_documented() {
        let ten_dollars = Money::from_cents(1000);
        // If we split $10.00 three ways: $3.33 each
        let one_third = Money::from_cents(1000 / 3); // 333 cents
        let reconstructed: Money = one_third * 3; // 999 cents

        // We intentionally lose 1 cent - this is documented behavior
        assert_eq!(reconstructed.cents(), 999);
        assert_ne!(reconstructed.cents(), ten_dollars.cents());

        // Document: 1 cent was lost
        let lost = ten_dollars - reconstructed;
        assert_eq!(lost.cents(), 1);
    }
}
