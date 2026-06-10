//! Shared integer accounting math for Roshi vaults.

use crate::error::RoshiError;

pub const SHARE_DECIMALS: u8 = 9;
pub const BPS_DENOMINATOR: u16 = 10_000;

pub type MathResult<T> = Result<T, RoshiError>;

pub fn pow10(decimals: u8) -> MathResult<u128> {
    10u128
        .checked_pow(u32::from(decimals))
        .ok_or(RoshiError::InvalidDecimals)
}

pub fn mul_div_floor(lhs: u128, rhs: u128, denominator: u128) -> MathResult<u128> {
    if denominator == 0 {
        return Err(RoshiError::DivisionByZero);
    }

    lhs.checked_mul(rhs)
        .ok_or(RoshiError::Overflow)
        .map(|product| product / denominator)
}

pub fn mul_div_ceil(lhs: u128, rhs: u128, denominator: u128) -> MathResult<u128> {
    if denominator == 0 {
        return Err(RoshiError::DivisionByZero);
    }

    let product = lhs.checked_mul(rhs).ok_or(RoshiError::Overflow)?;
    let quotient = product / denominator;

    if product % denominator == 0 {
        Ok(quotient)
    } else {
        quotient.checked_add(1).ok_or(RoshiError::Overflow)
    }
}

pub fn checked_u64(value: u128) -> MathResult<u64> {
    u64::try_from(value).map_err(|_| RoshiError::ResultDoesNotFit)
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
        return Err(RoshiError::InvalidBps);
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
        return Err(RoshiError::InvalidVaultState);
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
        return Err(RoshiError::InvalidVaultState);
    }

    let assets = mul_div_floor(
        u128::from(shares),
        u128::from(total_assets),
        u128::from(total_shares),
    )?;
    checked_nonzero_u64(assets)
}

pub fn share_price_from_assets(total_assets: u64, total_shares: u64) -> MathResult<u64> {
    if total_shares == 0 {
        return Err(RoshiError::InvalidVaultState);
    }

    let share_scale = pow10(SHARE_DECIMALS)?;
    let share_price = mul_div_floor(
        u128::from(total_assets),
        share_scale,
        u128::from(total_shares),
    )?;
    checked_u64(share_price)
}

pub fn performance_fee_for_nav(
    gross_total_assets: u64,
    total_shares: u64,
    high_watermark: u64,
    performance_fee_bps: u16,
) -> MathResult<(u64, u64, u64)> {
    validate_percentage_bps(performance_fee_bps)?;

    if total_shares == 0 {
        return Ok((0, gross_total_assets, high_watermark));
    }

    let gross_share_price = share_price_from_assets(gross_total_assets, total_shares)?;
    if high_watermark == 0 || gross_share_price <= high_watermark || performance_fee_bps == 0 {
        return Ok((0, gross_total_assets, high_watermark.max(gross_share_price)));
    }

    let share_scale = pow10(SHARE_DECIMALS)?;
    let high_watermark_assets = checked_u64(mul_div_ceil(
        u128::from(high_watermark),
        u128::from(total_shares),
        share_scale,
    )?)?;
    let profit_assets = gross_total_assets
        .checked_sub(high_watermark_assets)
        .ok_or(RoshiError::Overflow)?;
    let fee_assets = bps_floor(profit_assets, performance_fee_bps)?;
    let net_total_assets = gross_total_assets
        .checked_sub(fee_assets)
        .ok_or(RoshiError::Overflow)?;
    let net_share_price = share_price_from_assets(net_total_assets, total_shares)?;

    Ok((
        fee_assets,
        net_total_assets,
        high_watermark.max(net_share_price),
    ))
}

fn checked_nonzero_u64(value: u128) -> MathResult<u64> {
    let value = checked_u64(value)?;
    if value == 0 {
        return Err(RoshiError::ZeroOutput);
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
        assert_eq!(pow10(39), Err(RoshiError::InvalidDecimals));
    }

    #[test]
    fn mul_div_floor_and_ceil_handle_boundaries() {
        assert_eq!(mul_div_floor_u64(10, 2, 4), Ok(5));
        assert_eq!(mul_div_floor_u64(10, 2, 6), Ok(3));
        assert_eq!(mul_div_ceil_u64(10, 2, 4), Ok(5));
        assert_eq!(mul_div_ceil_u64(10, 2, 6), Ok(4));
        assert_eq!(mul_div_floor_u64(1, 1, 0), Err(RoshiError::DivisionByZero));
        assert_eq!(mul_div_ceil_u64(1, 1, 0), Err(RoshiError::DivisionByZero));
    }

    #[test]
    fn mul_div_rejects_overflow_and_downcast() {
        assert_eq!(mul_div_floor(u128::MAX, 2, 1), Err(RoshiError::Overflow));
        assert_eq!(
            mul_div_floor_u64(u64::MAX, u64::MAX, 1),
            Err(RoshiError::ResultDoesNotFit)
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
        assert_eq!(validate_percentage_bps(10_001), Err(RoshiError::InvalidBps));
    }

    #[test]
    fn normalizes_oracle_values_into_base_atoms() {
        assert_eq!(
            base_atoms_from_asset_atoms(1_000_000, 2_500_000_000, 9),
            Ok(2_500_000)
        );
        assert_eq!(
            base_atoms_from_asset_atoms(u64::MAX, u128::from(u64::MAX), 0),
            Err(RoshiError::ResultDoesNotFit)
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
            Err(RoshiError::InvalidDecimals)
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
            Err(RoshiError::ZeroOutput)
        );
    }

    #[test]
    fn initial_share_scale_rejects_invalid_decimals_and_downcast_overflow() {
        assert_eq!(
            initial_shares_from_base_atoms(1, 39),
            Err(RoshiError::InvalidDecimals)
        );
        assert_eq!(
            initial_shares_from_base_atoms(u64::MAX, 0),
            Err(RoshiError::ResultDoesNotFit)
        );
    }

    #[test]
    fn deposit_shares_are_floor_rounded_and_monotonic() {
        assert_eq!(shares_for_deposit(100, 1_000, 10_000), Ok(1_000));
        assert_eq!(shares_for_deposit(101, 1_000, 10_000), Ok(1_010));
        assert_eq!(
            shares_for_deposit(1, 1_000, 100),
            Err(RoshiError::ZeroOutput)
        );
        assert_eq!(
            shares_for_deposit(1, 0, 100),
            Err(RoshiError::InvalidVaultState)
        );
        assert_eq!(
            shares_for_deposit(1, 100, 0),
            Err(RoshiError::InvalidVaultState)
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
        assert_eq!(
            assets_for_redeem(1, 100, 1_000),
            Err(RoshiError::ZeroOutput)
        );
        assert_eq!(
            assets_for_redeem(1, 0, 100),
            Err(RoshiError::InvalidVaultState)
        );
        assert_eq!(
            assets_for_redeem(101, 100, 100),
            Err(RoshiError::InvalidVaultState)
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
        assert_eq!(
            assets_for_redeem(shares, 3, 10),
            Err(RoshiError::ZeroOutput)
        );

        let shares = shares_for_deposit(100, 333, 1_000).unwrap();
        let assets = assets_for_redeem(shares, 333, 1_000).unwrap();
        assert!(assets <= 100);
    }

    #[test]
    fn share_price_uses_fixed_share_scale() {
        assert_eq!(
            share_price_from_assets(1_000_000, 1_000_000_000),
            Ok(1_000_000)
        );
        assert_eq!(
            share_price_from_assets(1_100_000, 1_000_000_000),
            Ok(1_100_000)
        );
        assert_eq!(
            share_price_from_assets(1_000_000, 0),
            Err(RoshiError::InvalidVaultState)
        );
    }

    #[test]
    fn performance_fee_for_nav_accrues_on_high_watermark_gains() {
        assert_eq!(
            performance_fee_for_nav(1_100_000, 1_000_000_000, 1_000_000, 1_000),
            Ok((10_000, 1_090_000, 1_090_000))
        );
    }

    #[test]
    fn performance_fee_for_nav_sets_initial_high_watermark_without_fee() {
        assert_eq!(
            performance_fee_for_nav(1_000_000, 1_000_000_000, 0, 1_000),
            Ok((0, 1_000_000, 1_000_000))
        );
    }

    #[test]
    fn performance_fee_for_nav_keeps_high_watermark_on_drawdown() {
        assert_eq!(
            performance_fee_for_nav(900_000, 1_000_000_000, 1_000_000, 1_000),
            Ok((0, 900_000, 1_000_000))
        );
    }

    #[test]
    fn performance_fee_for_nav_ceil_rounds_high_watermark_assets() {
        assert_eq!(
            performance_fee_for_nav(2, 3, 333_333_334, 10_000),
            Ok((0, 2, 666_666_666))
        );
    }

    #[test]
    fn performance_fee_for_nav_floors_indivisible_accrual() {
        // Profit of 11 at 1_000 bps is 1.1 units of fee. The fee must *floor*
        // (charge 1, in the depositor's favour), never ceil to 2. The divisible
        // example above can't tell floor from ceil; this one pins the direction.
        assert_eq!(
            performance_fee_for_nav(1_000_011, 1_000_000_000, 1_000_000, 1_000),
            Ok((1, 1_000_010, 1_000_010))
        );
    }

    #[test]
    fn bps_ceil_never_exceeds_amount_exhaustive_over_bps() {
        // Ground truth: bitwuzla-via-CBMC-6.8 reported `ceil <= amount` as a
        // FAILURE, but its SMT2 backend crashes (smt2_conv invariant violation),
        // so that verdict is a solver artifact. This native check is exhaustive
        // over every valid bps at the boundary-relevant amounts.
        let amounts = [
            0u64,
            1,
            2,
            3,
            7,
            9_999,
            10_000,
            10_001,
            u64::MAX / 2,
            u64::MAX - 1,
            u64::MAX,
        ];
        for &amount in &amounts {
            for bps in 0..=BPS_DENOMINATOR {
                let ceil = bps_ceil(amount, bps).unwrap();
                assert!(ceil <= amount, "ceil {ceil} > amount {amount} at bps {bps}");
            }
        }
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
        fn prop_performance_fee_conserves_and_ratchets(
            gross in 0u64..=1_000_000_000,
            total_shares in 0u64..=1_000_000_000,
            high_watermark in 0u64..=1_000_000_000,
            bps in 0u16..=BPS_DENOMINATOR,
        ) {
            // These inputs are all in-domain, so the accrual must not error;
            // a swallowed `Err` would hide exactly the regression we want loud.
            let (fee, net, new_hwm) =
                performance_fee_for_nav(gross, total_shares, high_watermark, bps).unwrap();
            // Conservation: the fee is carved out of gross, nothing created.
            prop_assert!(fee <= gross);
            prop_assert_eq!(net, gross - fee);
            // The high-watermark only ever ratchets up.
            prop_assert!(new_hwm >= high_watermark);
            // No fee with no rate and no fee with no shares to charge against.
            if bps == 0 || total_shares == 0 {
                prop_assert_eq!(fee, 0);
            }
        }

        #[test]
        fn prop_performance_fee_is_monotonic_in_bps(
            gross in 0u64..=1_000_000_000,
            total_shares in 0u64..=1_000_000_000,
            high_watermark in 0u64..=1_000_000_000,
            bps_a in 0u16..=BPS_DENOMINATOR,
            bps_b in 0u16..=BPS_DENOMINATOR,
        ) {
            // A higher fee rate can never charge less on the same NAV report.
            // Forces the fee-charged branch to be meaningful without the test
            // re-deriving the fee formula (exact values live in the example tests).
            let (lo, hi) = if bps_a <= bps_b { (bps_a, bps_b) } else { (bps_b, bps_a) };
            let (fee_lo, ..) =
                performance_fee_for_nav(gross, total_shares, high_watermark, lo).unwrap();
            let (fee_hi, ..) =
                performance_fee_for_nav(gross, total_shares, high_watermark, hi).unwrap();
            prop_assert!(fee_hi >= fee_lo);
        }

    }
}
