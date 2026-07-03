//! Violation types and verification results.

/// A specific violation found during verification.
#[derive(Clone, Debug)]
pub struct Violation {
    pub kind: ViolationKind,
    pub details: String,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.details)
    }
}

/// All possible violation kinds across all 4 verification layers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViolationKind {
    // === Layer 1: Match verification (per-fill) ===
    /// Fill references a non-existent order.
    OrderNotFound,
    /// `fill_qty > order.max_fill`.
    QuantityExceedsMax,
    /// Fill price violates the order's limit price.
    PriceExceedsLimit,
    /// Same order filled multiple times.
    DuplicateFill,
    /// Negative welfare for a fill.
    NegativeWelfare,
    /// Computed welfare sum != reported total.
    WelfareMismatch,
    /// Market maker capital used exceeds budget.
    MmBudgetExceeded,
    /// Zero-quantity fill (diagnostic only).
    ZeroQuantityFill,
    /// Net position delta != 0 for a market (minting invariant broken).

    // === Layer 1: Match verification (market-level) ===
    /// A single-market fill's price does not match the clearing price.
    UniformClearingPriceViolation,
    /// `clearing_price_YES + clearing_price_NO != NANOS_PER_DOLLAR`.
    PriceComplementarityViolation,
    /// Sum of YES clearing prices in a group exceeds `NANOS_PER_DOLLAR`.
    MarketGroupConstraintViolation,
    /// Fill or order references a resolved/voided market.
    ResolvedMarketViolation,
    /// Duplicate order IDs in the witness.
    DuplicateOrderId,
    /// Conditional order filled but activation condition not met.
    ConditionalActivationViolation,

    // === Layer 2: Settlement verification ===
    /// Derived post-state balance is negative after settlement.
    NegativeBalance,
    /// Derived post-state position is negative after settlement.
    NegativePosition,
    /// Derived post-state balance does not match claimed post-state.
    SettlementBalanceMismatch,
    /// Derived post-state position does not match claimed post-state.
    SettlementPositionMismatch,
    /// Arithmetic overflow during settlement re-derivation.
    SettlementOverflow,
    /// Account present in post-state but not in pre-state (or vice versa).
    SettlementAccountMismatch,
    /// Position imbalance exists but no clearing price for the market.
    /// MINT cannot be priced without clearing prices.
    MintingWithoutClearingPrice,

    // === Layer 3: Block verification ===
    /// Recomputed state root does not match header.
    StateRootMismatch,
    /// Recomputed events root does not match header.
    EventRootMismatch,
    /// Recomputed parent hash does not match header.
    ParentHashMismatch,
    /// `header.height != previous.height + 1`.
    HeightNotConsecutive,
    /// `header.order_count` does not match actual count.
    OrderCountMismatch,
    /// `header.fill_count` does not match actual count.
    FillCountMismatch,
    /// Genesis block must have `parent_hash = [0; 32]`.
    GenesisParentHashNonZero,

    // === Layer 4: Order verification ===
    /// Buy order's account cannot cover worst-case cost.
    InsufficientBalance,
    /// Sell order's account lacks the position.
    InsufficientPosition,
    /// Intra-batch double-spend detected.
    IntraBatchDoubleSpend,
    /// Accepted order references an account missing from post-system-state.
    AcceptedOrderMissingAccount,
    /// Accepted order was not eligible at this block height.
    OrderExpiryViolation,
    /// Accepted order shape or quantity is not supported by production admission.
    InvalidOrder,
    /// A rejection is incorrect — order would have been valid.
    FalseRejection,
    /// A rejection reason does not match the actual validation failure.
    IncorrectRejectionReason,
}

/// Result of running one or more verification layers.
#[derive(Clone, Debug)]
pub struct VerificationResult {
    /// `true` if zero violations were found.
    pub valid: bool,
    /// All violations found (may span multiple layers).
    pub violations: Vec<Violation>,
    /// Statistics about what was checked.
    pub stats: VerificationStats,
}

impl VerificationResult {
    /// Create a result from a list of violations.
    pub fn from_violations(violations: Vec<Violation>) -> Self {
        Self {
            valid: violations.is_empty(),
            violations,
            stats: VerificationStats::default(),
        }
    }

    /// Merge another result into this one.
    pub fn merge(&mut self, other: VerificationResult) {
        if !other.valid {
            self.valid = false;
        }
        self.violations.extend(other.violations);
        self.stats.merge(other.stats);
    }
}

/// Statistics from verification.
#[derive(Clone, Debug, Default)]
pub struct VerificationStats {
    pub fills_checked: usize,
    pub orders_checked: usize,
    pub mm_constraints_checked: usize,
    pub computed_welfare: i64,
    pub reported_welfare: i64,
    /// Solver-reported minting/welfare adjustment in nanos.
    /// Live block solvency is checked by settlement replaying MINT account
    /// adjustments rather than by synthetic fills.
    pub minting_cost_nanos: i64,
    pub accounts_checked: usize,
    /// Average |sum_of_YES_prices - $1| across market groups, in nanos.
    /// Lower is better. 0 means perfect. Only meaningful when groups exist.
    pub market_group_avg_delta: Option<u64>,
}

impl VerificationStats {
    fn merge(&mut self, other: VerificationStats) {
        self.fills_checked += other.fills_checked;
        self.orders_checked += other.orders_checked;
        self.mm_constraints_checked += other.mm_constraints_checked;
        // Keep the last non-zero welfare values
        if other.computed_welfare != 0 {
            self.computed_welfare = other.computed_welfare;
        }
        if other.reported_welfare != 0 {
            self.reported_welfare = other.reported_welfare;
        }
        if other.minting_cost_nanos != 0 {
            self.minting_cost_nanos = other.minting_cost_nanos;
        }
        self.accounts_checked += other.accounts_checked;
        if other.market_group_avg_delta.is_some() {
            self.market_group_avg_delta = other.market_group_avg_delta;
        }
    }
}
