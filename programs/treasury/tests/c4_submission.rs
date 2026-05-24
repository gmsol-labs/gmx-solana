//! C4 Submission — F-001
//!
//! Treasury `swap_in_token` constraint uses `!is_deposit_allowed` instead of
//! `is_withdrawal_allowed`, allowing locked tokens to be swapped out.
//!
//! Run: cargo test --package gmsol-treasury --test c4_submission -- --nocapture

// ---------------------------------------------------------------------------
// Simulate TokenFlags — match the production bit layout:
//   bit 0: AllowDeposit
//   bit 1: AllowWithdrawal
// ---------------------------------------------------------------------------
struct TokenFlags(u8);

impl TokenFlags {
    fn new(deposit: bool, withdrawal: bool) -> Self {
        Self(
            (if deposit { 1u8 } else { 0u8 })
                | (if withdrawal { 2u8 } else { 0u8 }),
        )
    }
    fn allow_deposit(&self) -> bool {
        self.0 & 0x01 != 0
    }
    fn allow_withdrawal(&self) -> bool {
        self.0 & 0x02 != 0
    }
}

// ---------------------------------------------------------------------------
// The two constraint expressions as they appear in swap.rs#L38-L39
// ---------------------------------------------------------------------------

/// Current (buggy) constraint: `!is_deposit_allowed(swap_in_token)`
fn buggy_constraint(flags: &TokenFlags) -> bool {
    !flags.allow_deposit()
}

/// Correct constraint: `is_withdrawal_allowed(swap_in_token)`
fn fixed_constraint(flags: &TokenFlags) -> bool {
    flags.allow_withdrawal()
}

// ===========================================================================
// Test A: Fully locked token (deposit=false, withdrawal=false)
// Intended: BLOCKED
// Actual:   ALLOWED   (BUG)
// ===========================================================================
#[test]
fn test_locked_token_bypass() {
    let flags = TokenFlags::new(false, false); // fully locked

    assert_eq!(buggy_constraint(&flags), true,
        "BUG: !allow_deposit(locked) = TRUE -> locked token PASSES the constraint"
    );
    assert_eq!(fixed_constraint(&flags), false,
        "FIX: allow_withdrawal(locked) = FALSE -> locked token is CORRECTLY blocked"
    );

    println!("[TEST A] Fully locked token (deposit=false, withdrawal=false)");
    println!("  BUG: !allow_deposit     = {}  -- ALLOWED (should be blocked)", buggy_constraint(&flags));
    println!("  FIX: allow_withdrawal   = {}  -- BLOCKED (correct)",         fixed_constraint(&flags));
}

// ===========================================================================
// Test B: Fully enabled token (deposit=true, withdrawal=true)
// Intended: ALLOWED
// Actual:   BLOCKED   (BUG)
// ===========================================================================
#[test]
fn test_enabled_token_rejection() {
    let flags = TokenFlags::new(true, true); // fully enabled

    assert_eq!(buggy_constraint(&flags), false,
        "BUG: !allow_deposit(enabled) = FALSE -> enabled token is REJECTED"
    );
    assert_eq!(fixed_constraint(&flags), true,
        "FIX: allow_withdrawal(enabled) = TRUE -> enabled token CORRECTLY passes"
    );

    println!("[TEST B] Fully enabled token (deposit=true, withdrawal=true)");
    println!("  BUG: !allow_deposit     = {}  -- BLOCKED (should be allowed)", buggy_constraint(&flags));
    println!("  FIX: allow_withdrawal   = {}  -- ALLOWED (correct)",          fixed_constraint(&flags));
}

// ===========================================================================
// Test C: All 4 permutations
// ===========================================================================
#[test]
fn test_all_permutations() {
    let cases = [
        (false, false, "locked   "),
        (true,  false, "deposit  "),
        (false, true,  "withdraw "),
        (true,  true,  "enabled  "),
    ];

    println!("[TEST C] All flag permutations");
    println!("Deposit  Withdraw  !allow_dep (buggy)  allow_withdr (fixed)  Match");
    println!("-------  --------  ----------------  -------------------  -----");

    for (dep, wd, label) in cases {
        let flags = TokenFlags::new(dep, wd);
        let buggy = buggy_constraint(&flags);
        let fixed = fixed_constraint(&flags);
        let matches = buggy == fixed;
        println!(
            "{:<8} {:<8} {:<17} {:<20} {}",
            dep, wd, buggy, fixed,
            if matches { "OK" } else { "MISMATCH" }
        );
        if !matches {
            println!("  ^ buggy={}, should be {}", buggy, fixed);
        }
    }
}

// ===========================================================================
// Test D: Missing token (unwrap_or(false) vs ? propagation)
// ===========================================================================
#[test]
fn test_missing_token_scenario() {
    // When a token is NOT in the treasury vault config:
    //   is_deposit_allowed(token) -> Err(NotFound)
    //   .unwrap_or(false) -> false
    //   !false -> true -> ALLOWED
    //
    // With the fix:
    //   is_withdrawal_allowed(token) -> Err(NotFound)
    //   ? propagates as error -> REJECTED

    // Simulate the unwrap_or(false) path:
    let is_deposit_allowed_result: Result<bool, ()> = Err(()); // token not found
    let deposit_allowed = is_deposit_allowed_result.unwrap_or(false);
    let buggy_passes = !deposit_allowed;

    assert_eq!(buggy_passes, true,
        "BUG: missing token passes via unwrap_or(false) -> !false -> true"
    );

    // Simulate the `?` path (correct):
    let is_withdrawal_allowed_result: Result<bool, &str> = Err("NotFound");
    let fixed_passes = is_withdrawal_allowed_result.is_ok()
        && is_withdrawal_allowed_result.unwrap();

    assert_eq!(fixed_passes, false,
        "Missing token is correctly blocked when using `?` propagation"
    );

    println!("[TEST D] Missing token (not in config)");
    println!("  BUG: unwrap_or(false) -> !false = {}  -- ALLOWED", buggy_passes);
    println!("  FIX: ? propagates Err  -> {}              -- BLOCKED", fixed_passes);
}
