//! Shared integer accounting math for Roshi vaults.

pub const SHARE_DECIMALS: u8 = 9;
pub const BPS_DENOMINATOR: u16 = 10_000;

pub type MathResult<T> = Result<T, MathError>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MathError {
    DivisionByZero,
    InvalidBps,
    InvalidDecimals,
    InvalidVaultState,
    Overflow,
    ResultDoesNotFit,
    ZeroOutput,
}

pub fn pow10(decimals: u8) -> MathResult<u128> {
    10u128
        .checked_pow(u32::from(decimals))
        .ok_or(MathError::InvalidDecimals)
}

pub fn mul_div_floor(lhs: u128, rhs: u128, denominator: u128) -> MathResult<u128> {
    if denominator == 0 {
        return Err(MathError::DivisionByZero);
    }

    lhs.checked_mul(rhs)
        .ok_or(MathError::Overflow)
        .map(|product| product / denominator)
}

pub fn mul_div_ceil(lhs: u128, rhs: u128, denominator: u128) -> MathResult<u128> {
    if denominator == 0 {
        return Err(MathError::DivisionByZero);
    }

    let product = lhs.checked_mul(rhs).ok_or(MathError::Overflow)?;
    let quotient = product / denominator;

    if product % denominator == 0 {
        Ok(quotient)
    } else {
        quotient.checked_add(1).ok_or(MathError::Overflow)
    }
}

pub fn checked_u64(value: u128) -> MathResult<u64> {
    u64::try_from(value).map_err(|_| MathError::ResultDoesNotFit)
}

pub fn mul_div_floor_u64(lhs: u64, rhs: u64, denominator: u64) -> MathResult<u64> {
    let value = mul_div_floor(u128::from(lhs), u128::from(rhs), u128::from(denominator))?;
    checked_u64(value)
}

pub fn mul_div_ceil_u64(lhs: u64, rhs: u64, denominator: u64) -> MathResult<u64> {
    let value = mul_div_ceil(u128::from(lhs), u128::from(rhs), u128::from(denominator))?;
    checked_u64(value)
}

pub fn bps_floor(amount: u64, bps: u16) -> MathResult<u64> {
    mul_div_floor_u64(amount, u64::from(bps), u64::from(BPS_DENOMINATOR))
}

pub fn bps_ceil(amount: u64, bps: u16) -> MathResult<u64> {
    mul_div_ceil_u64(amount, u64::from(bps), u64::from(BPS_DENOMINATOR))
}

pub fn validate_percentage_bps(bps: u16) -> MathResult<()> {
    if bps > BPS_DENOMINATOR {
        return Err(MathError::InvalidBps);
    }

    Ok(())
}

pub fn base_atoms_from_asset_atoms(
    asset_atoms: u64,
    price_value: u128,
    price_decimals: u8,
) -> MathResult<u64> {
    let scale = pow10(price_decimals)?;
    let value = mul_div_floor(u128::from(asset_atoms), price_value, scale)?;
    checked_u64(value)
}

pub fn initial_shares_from_base_atoms(base_atoms: u64, base_decimals: u8) -> MathResult<u64> {
    let share_scale = pow10(SHARE_DECIMALS)?;
    let base_scale = pow10(base_decimals)?;
    let shares = mul_div_floor(u128::from(base_atoms), share_scale, base_scale)?;
    checked_nonzero_u64(shares)
}

pub fn shares_for_deposit(
    base_atoms: u64,
    total_assets: u64,
    total_shares: u64,
) -> MathResult<u64> {
    if total_assets == 0 || total_shares == 0 {
        return Err(MathError::InvalidVaultState);
    }

    let shares = mul_div_floor(
        u128::from(base_atoms),
        u128::from(total_shares),
        u128::from(total_assets),
    )?;
    checked_nonzero_u64(shares)
}

pub fn assets_for_redeem(shares: u64, total_assets: u64, total_shares: u64) -> MathResult<u64> {
    if total_assets == 0 || total_shares == 0 || shares > total_shares {
        return Err(MathError::InvalidVaultState);
    }

    let assets = mul_div_floor(
        u128::from(shares),
        u128::from(total_assets),
        u128::from(total_shares),
    )?;
    checked_nonzero_u64(assets)
}

pub fn nav_delta_within_bps(
    old_total_assets: u64,
    new_total_assets: u64,
    max_change_bps: u16,
) -> MathResult<bool> {
    let delta = old_total_assets.abs_diff(new_total_assets);
    let max_delta = mul_div_floor(
        u128::from(old_total_assets),
        u128::from(max_change_bps),
        u128::from(BPS_DENOMINATOR),
    )?;

    Ok(u128::from(delta) <= max_delta)
}

fn checked_nonzero_u64(value: u128) -> MathResult<u64> {
    let value = checked_u64(value)?;
    if value == 0 {
        return Err(MathError::ZeroOutput);
    }

    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn pow10_rejects_unsupported_decimals() {
        assert!(pow10(38).is_ok());
        assert_eq!(pow10(39), Err(MathError::InvalidDecimals));
    }

    #[test]
    fn mul_div_floor_and_ceil_handle_boundaries() {
        assert_eq!(mul_div_floor_u64(10, 2, 4), Ok(5));
        assert_eq!(mul_div_floor_u64(10, 2, 6), Ok(3));
        assert_eq!(mul_div_ceil_u64(10, 2, 4), Ok(5));
        assert_eq!(mul_div_ceil_u64(10, 2, 6), Ok(4));
        assert_eq!(mul_div_floor_u64(1, 1, 0), Err(MathError::DivisionByZero));
        assert_eq!(mul_div_ceil_u64(1, 1, 0), Err(MathError::DivisionByZero));
    }

    #[test]
    fn mul_div_rejects_overflow_and_downcast() {
        assert_eq!(mul_div_floor(u128::MAX, 2, 1), Err(MathError::Overflow));
        assert_eq!(
            mul_div_floor_u64(u64::MAX, u64::MAX, 1),
            Err(MathError::ResultDoesNotFit)
        );
    }

    #[test]
    fn bps_helpers_use_standard_denominator() {
        assert_eq!(bps_floor(101, 100), Ok(1));
        assert_eq!(bps_ceil(101, 100), Ok(2));
        assert_eq!(bps_floor(42, 10_001), Ok(42));
        assert_eq!(bps_ceil(42, 10_001), Ok(43));
    }

    #[test]
    fn percentage_bps_validation_caps_at_full_percentage() {
        assert_eq!(validate_percentage_bps(0), Ok(()));
        assert_eq!(validate_percentage_bps(10_000), Ok(()));
        assert_eq!(validate_percentage_bps(10_001), Err(MathError::InvalidBps));
    }

    #[test]
    fn normalizes_oracle_values_into_base_atoms() {
        assert_eq!(
            base_atoms_from_asset_atoms(1_000_000, 2_500_000_000, 9),
            Ok(2_500_000)
        );
        assert_eq!(
            base_atoms_from_asset_atoms(u64::MAX, u128::from(u64::MAX), 0),
            Err(MathError::ResultDoesNotFit)
        );
    }

    #[test]
    fn normalization_can_round_to_zero_without_failing() {
        assert_eq!(base_atoms_from_asset_atoms(0, 1_000_000_000, 9), Ok(0));
        assert_eq!(base_atoms_from_asset_atoms(1, 1, 9), Ok(0));
    }

    #[test]
    fn normalization_rejects_invalid_price_decimals() {
        assert_eq!(
            base_atoms_from_asset_atoms(1, 1, 39),
            Err(MathError::InvalidDecimals)
        );
    }

    #[test]
    fn initial_share_scale_uses_fixed_share_decimals_and_base_decimals() {
        assert_eq!(
            initial_shares_from_base_atoms(1_000_000, 6),
            Ok(1_000_000_000)
        );
        assert_eq!(
            initial_shares_from_base_atoms(1_000_000_000, 9),
            Ok(1_000_000_000)
        );
        assert_eq!(
            initial_shares_from_base_atoms(1, 12),
            Err(MathError::ZeroOutput)
        );
    }

    #[test]
    fn initial_share_scale_rejects_invalid_decimals_and_downcast_overflow() {
        assert_eq!(
            initial_shares_from_base_atoms(1, 39),
            Err(MathError::InvalidDecimals)
        );
        assert_eq!(
            initial_shares_from_base_atoms(u64::MAX, 0),
            Err(MathError::ResultDoesNotFit)
        );
    }

    #[test]
    fn deposit_shares_are_floor_rounded_and_monotonic() {
        assert_eq!(shares_for_deposit(100, 1_000, 10_000), Ok(1_000));
        assert_eq!(shares_for_deposit(101, 1_000, 10_000), Ok(1_010));
        assert_eq!(
            shares_for_deposit(1, 1_000, 100),
            Err(MathError::ZeroOutput)
        );
        assert_eq!(
            shares_for_deposit(1, 0, 100),
            Err(MathError::InvalidVaultState)
        );
        assert_eq!(
            shares_for_deposit(1, 100, 0),
            Err(MathError::InvalidVaultState)
        );
    }

    #[test]
    fn deposit_shares_preserve_exact_proportions() {
        assert_eq!(shares_for_deposit(250, 1_000, 4_000), Ok(1_000));
        assert_eq!(shares_for_deposit(333, 999, 3_000), Ok(1_000));
    }

    #[test]
    fn redeem_assets_are_floor_rounded_and_cannot_overpay() {
        assert_eq!(assets_for_redeem(1_000, 1_000, 10_000), Ok(100));
        assert_eq!(assets_for_redeem(1_010, 1_000, 10_000), Ok(101));
        assert_eq!(assets_for_redeem(1, 100, 1_000), Err(MathError::ZeroOutput));
        assert_eq!(
            assets_for_redeem(1, 0, 100),
            Err(MathError::InvalidVaultState)
        );
        assert_eq!(
            assets_for_redeem(101, 100, 100),
            Err(MathError::InvalidVaultState)
        );
    }

    #[test]
    fn redeeming_all_shares_returns_all_assets() {
        assert_eq!(assets_for_redeem(10_000, 1_000, 10_000), Ok(1_000));
        assert_eq!(assets_for_redeem(u64::MAX, 123, u64::MAX), Ok(123));
    }

    #[test]
    fn deposit_redeem_round_trip_does_not_overpay() {
        let shares = shares_for_deposit(1, 3, 10).unwrap();
        assert_eq!(shares, 3);
        assert_eq!(assets_for_redeem(shares, 3, 10), Err(MathError::ZeroOutput));

        let shares = shares_for_deposit(100, 333, 1_000).unwrap();
        let assets = assets_for_redeem(shares, 333, 1_000).unwrap();
        assert!(assets <= 100);
    }

    #[test]
    fn nav_delta_guardrail_uses_floor_bps() {
        assert_eq!(nav_delta_within_bps(1_000, 1_100, 1_000), Ok(true));
        assert_eq!(nav_delta_within_bps(1_000, 1_101, 1_000), Ok(false));
        assert_eq!(nav_delta_within_bps(0, 0, 1_000), Ok(true));
        assert_eq!(nav_delta_within_bps(0, 1, 1_000), Ok(false));
        assert_eq!(nav_delta_within_bps(100, 700, 60_000), Ok(true));
        assert_eq!(
            nav_delta_within_bps(u64::MAX, u64::MAX / 2, 60_000),
            Ok(true)
        );
    }

    #[test]
    fn nav_delta_guardrail_handles_large_boundary_values() {
        let old = u64::MAX;
        let max_delta = (u128::from(old) / u128::from(BPS_DENOMINATOR)) as u64;

        assert_eq!(nav_delta_within_bps(old, old - max_delta, 1), Ok(true));
        assert_eq!(nav_delta_within_bps(old, old - max_delta - 1, 1), Ok(false));

        let old = u64::MAX / 2;
        let max_delta = (u128::from(old) / u128::from(BPS_DENOMINATOR)) as u64;

        assert_eq!(nav_delta_within_bps(old, old + max_delta, 1), Ok(true));
        assert_eq!(nav_delta_within_bps(old, old + max_delta + 1, 1), Ok(false));
    }

    #[test]
    fn withdrawal_buffer_targets_can_round_up() {
        assert_eq!(bps_ceil(1_001, 100), Ok(11));
        assert_eq!(bps_floor(1_001, 100), Ok(10));
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn prop_floor_and_ceil_bound_exact_value(
            lhs in any::<u64>(),
            rhs in any::<u64>(),
            denominator in 1u64..=u64::MAX,
        ) {
            let product = u128::from(lhs) * u128::from(rhs);
            let denominator = u128::from(denominator);

            let floor = mul_div_floor(u128::from(lhs), u128::from(rhs), denominator).unwrap();
            let ceil = mul_div_ceil(u128::from(lhs), u128::from(rhs), denominator).unwrap();

            prop_assert!(floor <= ceil);
            prop_assert!(ceil <= floor + 1);
            prop_assert!(floor * denominator <= product);
            prop_assert!(product < (floor + 1) * denominator);
            prop_assert!(ceil * denominator >= product);
        }

        #[test]
        fn prop_bps_floor_and_ceil_are_ordered(
            amount in any::<u64>(),
            bps in 0u16..=BPS_DENOMINATOR,
        ) {
            let floor = bps_floor(amount, bps).unwrap();
            let ceil = bps_ceil(amount, bps).unwrap();

            prop_assert!(floor <= ceil);
            prop_assert!(ceil <= floor + 1);
            prop_assert!(ceil <= amount);
        }

        #[test]
        fn prop_deposit_shares_are_monotonic(
            total_assets in 1u64..=1_000_000_000,
            total_shares in 1u64..=1_000_000_000,
            base_atoms in 1u64..=1_000_000_000,
            extra_atoms in 0u64..=1_000_000_000,
        ) {
            let larger_base_atoms = base_atoms.saturating_add(extra_atoms);
            let smaller = shares_for_deposit(base_atoms, total_assets, total_shares);
            let larger = shares_for_deposit(larger_base_atoms, total_assets, total_shares);

            if let (Ok(smaller), Ok(larger)) = (smaller, larger) {
                prop_assert!(larger >= smaller);
            }
        }

        #[test]
        fn prop_redeem_assets_are_monotonic(
            total_assets in 1u64..=1_000_000_000,
            total_shares in 1u64..=1_000_000_000,
            share_seed in any::<u64>(),
            extra_seed in any::<u64>(),
        ) {
            let smaller_shares = 1 + (share_seed % total_shares);
            let remaining = total_shares - smaller_shares;
            let larger_shares = smaller_shares + (extra_seed % (remaining + 1));

            let smaller = assets_for_redeem(smaller_shares, total_assets, total_shares);
            let larger = assets_for_redeem(larger_shares, total_assets, total_shares);

            if let (Ok(smaller), Ok(larger)) = (smaller, larger) {
                prop_assert!(larger >= smaller);
            }
        }

        #[test]
        fn prop_deposit_then_redeem_never_overpays(
            base_atoms in 1u64..=1_000_000_000,
            total_assets in 1u64..=1_000_000_000,
            total_shares in 1u64..=1_000_000_000,
        ) {
            if let Ok(shares) = shares_for_deposit(base_atoms, total_assets, total_shares) {
                if let Ok(assets) = assets_for_redeem(shares, total_assets, total_shares) {
                    prop_assert!(assets <= base_atoms);
                }
            }
        }

        #[test]
        fn prop_full_redeem_returns_total_assets(
            total_assets in 1u64..=u64::MAX,
            total_shares in 1u64..=u64::MAX,
        ) {
            prop_assert_eq!(
                assets_for_redeem(total_shares, total_assets, total_shares),
                Ok(total_assets)
            );
        }

        #[test]
        fn prop_nav_delta_matches_floor_bps_guardrail(
            old_total_assets in any::<u64>(),
            new_total_assets in any::<u64>(),
            max_change_bps in any::<u16>(),
        ) {
            let delta = u128::from(old_total_assets.abs_diff(new_total_assets));
            let max_delta = u128::from(old_total_assets)
                * u128::from(max_change_bps)
                / u128::from(BPS_DENOMINATOR);

            prop_assert_eq!(
                nav_delta_within_bps(old_total_assets, new_total_assets, max_change_bps).unwrap(),
                delta <= max_delta
            );
        }
    }
}
