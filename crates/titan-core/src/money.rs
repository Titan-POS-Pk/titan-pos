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

    /// Adds two amounts, returning `None` on i64 overflow.
    #[inline]
    pub const fn checked_add(self, other: Self) -> Option<Self> {
        match self.0.checked_add(other.0) {
            Some(cents) => Some(Money(cents)),
            None => None,
        }
    }

    /// Subtracts `other` from `self`, returning `None` on i64 overflow.
    #[inline]
    pub const fn checked_sub(self, other: Self) -> Option<Self> {
        match self.0.checked_sub(other.0) {
            Some(cents) => Some(Money(cents)),
            None => None,
        }
    }

    /// Multiplies by a quantity, returning `None` on i64 overflow.
    #[inline]
    pub const fn checked_multiply_quantity(self, qty: i64) -> Option<Self> {
        match self.0.checked_mul(qty) {
            Some(cents) => Some(Money(cents)),
            None => None,
        }
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
    /// │                                                                     │
    /// │  It is also SIGN-SYMMETRIC: tax(-x) == -tax(x). A refund returns   │
    /// │  exactly what the sale charged. (See `div_round_half_even`.)       │
    /// └─────────────────────────────────────────────────────────────────────┘
    /// ```
    ///
    /// ## Implementation
    /// `div_round_half_even(amount_cents * bps, 10_000)`.
    ///
    /// The intermediate product is computed in i128 so a large line total
    /// cannot wrap; the quotient is then range-checked back into i64.
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
    /// // $10.00 × 8.25% = exactly 82.5 cents.
    /// // Half-to-even breaks the tie toward the even neighbour: 82, not 83.
    /// assert_eq!(tax.cents(), 82);
    ///
    /// // The matching refund moves the same number of cents back.
    /// let refund_tax = Money::from_cents(-1000).calculate_tax(rate);
    /// assert_eq!(refund_tax.cents(), -82);
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
        let numerator = self.0 as i128 * rate.bps() as i128;
        let tax_cents = div_round_half_even(numerator, BPS_DENOMINATOR);
        Money::from_cents(clamp_to_i64(tax_cents))
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
    ///
    /// ## Panics
    /// On i64 overflow, in every build profile. Use
    /// [`Money::checked_multiply_quantity`] where the quantity is untrusted.
    /// A wrapped total is a wrong total that looks plausible; a panic is not.
    #[inline]
    pub const fn multiply_quantity(&self, qty: i64) -> Self {
        match self.0.checked_mul(qty) {
            Some(cents) => Money(cents),
            None => panic!("Money overflow in multiply_quantity"),
        }
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
        // A discount above 100% would flip the sign of the line and hand money
        // to the customer. Clamp instead: the most you can take off is all of it.
        let discount_bps = discount_bps.min(BPS_DENOMINATOR as u32);

        let numerator = self.0 as i128 * discount_bps as i128;
        let discount_amount = div_round_half_even(numerator, BPS_DENOMINATOR);
        Money::from_cents(clamp_to_i64(self.0 as i128 - discount_amount))
    }
}

// =============================================================================
// Rounding
// =============================================================================

/// Basis-point denominator: 10_000 bps == 100%.
const BPS_DENOMINATOR: i128 = 10_000;

/// Divides `numerator / denominator`, rounding ties to the nearest even
/// integer (banker's rounding, IEEE 754 `roundTiesToEven`).
///
/// ## Why not `(n + d/2) / d`
/// That idiom — which this codebase used until it was measured — is round-half-
/// *up*, and Rust's integer division truncates toward zero rather than
/// flooring. The two errors do not cancel:
///
/// ```text
///   amount    exact      (n + 5000) / 10000     half-to-even
///   ────────  ─────────  ────────────────────   ────────────
///    $10.00 →   82.5 →   83  (rounded up)         82
///   -$10.00 → - 82.5 → - 82  (truncated toward 0) -82
///                       ^^^^ 1¢ short on the refund
/// ```
///
/// A POS charges tax on the sale and returns it on the refund. Under half-up
/// the two legs disagree by a cent on every tie, and the residue accumulates
/// in the tax account — the exact systematic bias banker's rounding exists to
/// prevent.
///
/// ## Method
/// Uses Euclidean division so the remainder is always non-negative, which makes
/// the tie test identical for positive and negative numerators:
///
/// - `2 * remainder > denominator` → away from the floor
/// - `2 * remainder < denominator` → toward the floor
/// - `2 * remainder == denominator` → pick whichever of the two neighbours is even
///
/// `denominator` must be positive.
#[inline]
fn div_round_half_even(numerator: i128, denominator: i128) -> i128 {
    debug_assert!(denominator > 0, "denominator must be positive");

    let floor = numerator.div_euclid(denominator);
    let remainder = numerator.rem_euclid(denominator); // always 0..denominator

    match (remainder * 2).cmp(&denominator) {
        std::cmp::Ordering::Greater => floor + 1,
        std::cmp::Ordering::Less => floor,
        // Exact tie: `floor` and `floor + 1` are consecutive, so exactly one
        // of them is even. Take that one.
        std::cmp::Ordering::Equal => {
            if floor % 2 == 0 {
                floor
            } else {
                floor + 1
            }
        }
    }
}

/// Narrows an i128 cent value back to i64, saturating at the bounds.
///
/// Reachable only with absurd inputs (a line total near `i64::MAX` combined
/// with a rate above 100%). Saturating beats the `as i64` truncation this
/// replaced, which silently wrapped a huge positive total into a negative one.
#[inline]
fn clamp_to_i64(cents: i128) -> i64 {
    cents.clamp(i64::MIN as i128, i64::MAX as i128) as i64
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
///
/// ## Panics
/// On i64 overflow, in release builds as well as debug. Plain `+` on i64 wraps
/// silently in release, which would turn an overlarge total into a negative one
/// that the rest of the system reads as a refund. Use [`Money::checked_add`]
/// when the inputs are not bounded by the cart limits.
impl Add for Money {
    type Output = Self;

    #[inline]
    fn add(self, other: Self) -> Self {
        self.checked_add(other).expect("Money overflow in add")
    }
}

/// Addition assignment (+=). Panics on overflow, as [`Add`] does.
impl AddAssign for Money {
    #[inline]
    fn add_assign(&mut self, other: Self) {
        *self = *self + other;
    }
}

/// Subtraction of two Money values. Panics on overflow, as [`Add`] does.
impl Sub for Money {
    type Output = Self;

    #[inline]
    fn sub(self, other: Self) -> Self {
        self.checked_sub(other).expect("Money overflow in sub")
    }
}

/// Subtraction assignment (-=). Panics on overflow, as [`Add`] does.
impl SubAssign for Money {
    #[inline]
    fn sub_assign(&mut self, other: Self) {
        *self = *self - other;
    }
}

/// Multiplication by integer (for quantity calculations).
/// Panics on overflow, as [`Add`] does.
impl Mul<i32> for Money {
    type Output = Self;

    #[inline]
    fn mul(self, qty: i32) -> Self {
        self.multiply_quantity(qty as i64)
    }
}

/// Multiplication by i64. Panics on overflow, as [`Add`] does.
impl Mul<i64> for Money {
    type Output = Self;

    #[inline]
    fn mul(self, qty: i64) -> Self {
        self.multiply_quantity(qty)
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
        // $10.00 at 8.25% = exactly 82.5 cents.
        // Half-to-even rounds the tie down to 82 (82 is even, 83 is odd).
        let amount = Money::from_cents(1000);
        let rate = TaxRate::from_bps(825);
        let tax = amount.calculate_tax(rate);
        assert_eq!(tax.cents(), 82);
    }

    /// A sale and its refund must move the same number of cents.
    ///
    /// Regression test. The previous implementation was
    /// `(amount * bps + 5000) / 10000`, which is round-half-*up*, and integer
    /// division in Rust truncates toward zero. On a $10.00 sale at 8.25% that
    /// charged 83 cents, but the matching -$10.00 refund returned only 82:
    /// the bias did not cancel, so every refunded transaction leaked a cent
    /// into the tax account.
    #[test]
    fn test_tax_is_symmetric_between_sale_and_refund() {
        let rate = TaxRate::from_bps(825); // 8.25%

        // Sweep amounts that land on and around the .5 tie.
        for cents in 1..=2_000i64 {
            let sale_tax = Money::from_cents(cents).calculate_tax(rate);
            let refund_tax = Money::from_cents(-cents).calculate_tax(rate);
            assert_eq!(
                sale_tax.cents(),
                -refund_tax.cents(),
                "asymmetric tax at {} cents: sale={} refund={}",
                cents,
                sale_tax.cents(),
                refund_tax.cents()
            );
        }
    }

    /// The exact tie cases the doc comment promises: 0.5 → 0, 1.5 → 2,
    /// 2.5 → 2, -0.5 → 0, -1.5 → -2.
    ///
    /// A 5% rate makes `amount_cents * 500 / 10000` land exactly on `.5`
    /// whenever `amount_cents` is an odd multiple of 10.
    #[test]
    fn test_tax_ties_round_half_to_even() {
        let rate = TaxRate::from_bps(500); // 5%

        // (amount_cents, exact quotient, expected half-to-even result)
        let cases = [
            (10i64, "0.5", 0i64),
            (30, "1.5", 2),
            (50, "2.5", 2),
            (70, "3.5", 4),
            (90, "4.5", 4),
            (-10, "-0.5", 0),
            (-30, "-1.5", -2),
            (-50, "-2.5", -2),
            (-70, "-3.5", -4),
        ];

        for (cents, quotient, expected) in cases {
            let tax = Money::from_cents(cents).calculate_tax(rate);
            assert_eq!(
                tax.cents(),
                expected,
                "{} cents at 5% = {} should round to {}, got {}",
                cents,
                quotient,
                expected,
                tax.cents()
            );
        }
    }

    /// Non-tie values must still round to nearest in both directions.
    #[test]
    fn test_tax_non_ties_round_to_nearest() {
        let rate = TaxRate::from_bps(100); // 1%

        // 1% of 149 cents = 1.49 → 1; of 151 cents = 1.51 → 2
        assert_eq!(Money::from_cents(149).calculate_tax(rate).cents(), 1);
        assert_eq!(Money::from_cents(151).calculate_tax(rate).cents(), 2);
        assert_eq!(Money::from_cents(-149).calculate_tax(rate).cents(), -1);
        assert_eq!(Money::from_cents(-151).calculate_tax(rate).cents(), -2);
    }

    /// Half-to-even exists to keep the rounding error centred on zero.
    /// Half-up drifts; this asserts the drift is gone.
    #[test]
    fn test_tax_rounding_has_no_systematic_bias() {
        let rate = TaxRate::from_bps(500); // 5%: every odd multiple of 10 is a tie

        let mut rounded_up = 0i64;
        let mut rounded_down = 0i64;

        // Walk every tie in the range: 10, 30, 50, ... 1990
        for cents in (10..2_000).step_by(20) {
            let exact_tenths = cents * 500 / 1_000; // tax * 10, exact
            let tax = Money::from_cents(cents).calculate_tax(rate).cents();
            if tax * 10 > exact_tenths {
                rounded_up += 1;
            } else {
                rounded_down += 1;
            }
        }

        assert_eq!(
            rounded_up, rounded_down,
            "ties should split evenly up/down, got {} up vs {} down",
            rounded_up, rounded_down
        );
    }

    #[test]
    fn test_tax_does_not_overflow_on_large_amounts() {
        // A i64::MAX-cent line total is nonsense in a store, but it must not
        // wrap into a negative tax.
        let huge = Money::from_cents(i64::MAX);
        let tax = huge.calculate_tax(TaxRate::from_bps(825));
        assert!(tax.is_positive());
        assert!(tax.cents() < huge.cents());
    }

    #[test]
    fn test_percentage_discount() {
        let subtotal = Money::from_cents(10000); // $100.00
        let discounted = subtotal.apply_percentage_discount(1000); // 10%
        assert_eq!(discounted.cents(), 9000); // $90.00
    }

    #[test]
    fn test_percentage_discount_uses_same_rounding_as_tax() {
        // 5% of 10 cents = exactly 0.5 → 0 under half-to-even, so nothing
        // comes off. Under the old half-up rule this took a cent.
        assert_eq!(
            Money::from_cents(10).apply_percentage_discount(500).cents(),
            10
        );
        // 5% of 30 cents = exactly 1.5 → 2.
        assert_eq!(
            Money::from_cents(30).apply_percentage_discount(500).cents(),
            28
        );
    }

    #[test]
    fn test_percentage_discount_is_symmetric_for_refunds() {
        for cents in 1..=500i64 {
            let sale = Money::from_cents(cents).apply_percentage_discount(1234);
            let refund = Money::from_cents(-cents).apply_percentage_discount(1234);
            assert_eq!(
                sale.cents(),
                -refund.cents(),
                "asymmetric at {} cents",
                cents
            );
        }
    }

    #[test]
    fn test_percentage_discount_cannot_exceed_full_amount() {
        // A >100% discount must not turn into money owed to the customer.
        let subtotal = Money::from_cents(10_000);
        assert_eq!(subtotal.apply_percentage_discount(10_000).cents(), 0);
        assert_eq!(subtotal.apply_percentage_discount(15_000).cents(), 0);
    }

    // -------------------------------------------------------------------------
    // Overflow
    // -------------------------------------------------------------------------

    #[test]
    fn test_checked_arithmetic_reports_overflow() {
        let max = Money::from_cents(i64::MAX);
        assert_eq!(max.checked_add(Money::from_cents(1)), None);
        assert_eq!(
            Money::from_cents(i64::MIN).checked_sub(Money::from_cents(1)),
            None
        );
        assert_eq!(max.checked_multiply_quantity(2), None);

        assert_eq!(
            Money::from_cents(100).checked_add(Money::from_cents(1)),
            Some(Money::from_cents(101))
        );
        assert_eq!(
            Money::from_cents(299).checked_multiply_quantity(3),
            Some(Money::from_cents(897))
        );
    }

    /// In release builds `i64 * i64` wraps silently. For money that turns a
    /// huge total into a negative one, which a POS would happily accept as a
    /// refund. The operators panic instead, in every profile.
    #[test]
    #[should_panic(expected = "Money overflow")]
    fn test_multiply_quantity_panics_instead_of_wrapping() {
        let _ = Money::from_cents(i64::MAX).multiply_quantity(2);
    }

    #[test]
    #[should_panic(expected = "Money overflow")]
    fn test_add_panics_instead_of_wrapping() {
        let _ = Money::from_cents(i64::MAX) + Money::from_cents(1);
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
