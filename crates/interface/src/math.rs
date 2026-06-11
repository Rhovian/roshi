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

/// The virtual share supply backing one virtual base atom:
/// `10^(SHARE_DECIMALS - base_decimals)`, the empty-vault mint ratio.
///
/// Deposits and redeems price against `(total_shares + offset, total_assets + 1)`
/// instead of the raw pair. This is the ERC-4626 virtual-offset defense against
/// donation/first-deposit share-price inflation: the virtual position absorbs
/// donated value, so inflating a later depositor's rounding loss costs the
/// attacker ~`offset` times that loss. It also makes pricing continuous through
/// the empty vault — the first deposit needs no special case.
///
/// Only virtual *shares* are scaled (virtual assets stay at 1): a virtual-asset
/// offset above 1 would let a full redeem price above `total_assets`. That
/// requires `base_decimals <= SHARE_DECIMALS`, enforced at vault initialization.
pub fn virtual_share_offset(base_decimals: u8) -> MathResult<u128> {
    let delta = SHARE_DECIMALS
        .checked_sub(base_decimals)
        .ok_or(RoshiError::InvalidDecimals)?;
    pow10(delta)
}

pub fn shares_for_deposit(
    base_atoms: u64,
    total_assets: u64,
    total_shares: u64,
    base_decimals: u8,
) -> MathResult<u64> {
    let virtual_shares = virtual_share_offset(base_decimals)?;
    let shares = mul_div_floor(
        u128::from(base_atoms),
        u128::from(total_shares) + virtual_shares,
        u128::from(total_assets) + 1,
    )?;
    checked_nonzero_u64(shares)
}

/// Floor-rounded base value of `shares`. Zero is a valid result: a dust
/// position can be worth less than one base atom, and withdrawal-ticket
/// strikes must price it (to nothing) rather than wedge. Immediate redemption
/// paths that must pay out should use [`assets_for_redeem`].
pub fn assets_for_shares(
    shares: u64,
    total_assets: u64,
    total_shares: u64,
    base_decimals: u8,
) -> MathResult<u64> {
    if shares > total_shares {
        return Err(RoshiError::InvalidVaultState);
    }

    let virtual_shares = virtual_share_offset(base_decimals)?;
    let assets = mul_div_floor(
        u128::from(shares),
        u128::from(total_assets) + 1,
        u128::from(total_shares) + virtual_shares,
    )?;
    checked_u64(assets)
}

pub fn assets_for_redeem(
    shares: u64,
    total_assets: u64,
    total_shares: u64,
    base_decimals: u8,
) -> MathResult<u64> {
    let assets = assets_for_shares(shares, total_assets, total_shares, base_decimals)?;
    if assets == 0 {
        return Err(RoshiError::ZeroOutput);
    }

    Ok(assets)
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
    fn virtual_share_offset_matches_empty_vault_mint_ratio() {
        assert_eq!(virtual_share_offset(6), Ok(1_000));
        assert_eq!(virtual_share_offset(9), Ok(1));
        assert_eq!(virtual_share_offset(10), Err(RoshiError::InvalidDecimals));
    }

    #[test]
    fn first_deposit_scales_base_atoms_to_share_decimals() {
        assert_eq!(shares_for_deposit(1_000_000, 0, 0, 6), Ok(1_000_000_000));
        assert_eq!(
            shares_for_deposit(1_000_000_000, 0, 0, 9),
            Ok(1_000_000_000)
        );
        assert_eq!(shares_for_deposit(0, 0, 0, 6), Err(RoshiError::ZeroOutput));
    }

    #[test]
    fn first_deposit_rejects_unsupported_decimals_and_downcast_overflow() {
        assert_eq!(
            shares_for_deposit(1, 0, 0, 10),
            Err(RoshiError::InvalidDecimals)
        );
        assert_eq!(
            shares_for_deposit(u64::MAX, 0, 0, 0),
            Err(RoshiError::ResultDoesNotFit)
        );
    }

    #[test]
    fn deposit_shares_are_exact_at_par_and_floor_otherwise() {
        // At par (supply/assets equals the virtual ratio) the offset cancels
        // and pricing is exact.
        assert_eq!(shares_for_deposit(100, 1_000, 1_000_000, 6), Ok(100_000));
        assert_eq!(shares_for_deposit(101, 1_000, 1_000_000, 6), Ok(101_000));
        // Off par the result floors (and the virtual position drags a
        // dust-sized pot toward par; at realistic pots the pull vanishes).
        assert_eq!(shares_for_deposit(100, 2_000, 1_000_000, 6), Ok(50_024));
        assert_eq!(
            shares_for_deposit(1, 1_000, 100, 9),
            Err(RoshiError::ZeroOutput)
        );
    }

    #[test]
    fn donation_inflation_costs_the_attacker_the_offset_multiple() {
        // Classic ERC-4626 inflation attack at 6 base decimals (offset 1000):
        // attacker seeds 1 atom, donates 10^9 atoms, and a NAV report folds the
        // donation into total_assets before the victim deposits 10^6 atoms.
        let attacker_shares = shares_for_deposit(1, 0, 0, 6).unwrap();
        assert_eq!(attacker_shares, 1_000);

        let donated_assets = 1 + 1_000_000_000;
        // The victim still mints (no zero-share grief)...
        let victim_shares =
            shares_for_deposit(1_000_000, donated_assets, attacker_shares, 6).unwrap();
        assert_eq!(victim_shares, 1);

        // ...and the virtual position absorbs the donation: the attacker's
        // claim comes back ~offset times further short than the victim's loss.
        let total_assets = donated_assets + 1_000_000;
        let total_shares = attacker_shares + victim_shares;
        let attacker_claim =
            assets_for_redeem(attacker_shares, total_assets, total_shares, 6).unwrap();
        let victim_claim = assets_for_redeem(victim_shares, total_assets, total_shares, 6).unwrap();
        let attacker_cost = donated_assets - attacker_claim;
        let victim_loss = 1_000_000 - victim_claim;
        assert!(victim_loss < 500_000);
        assert!(attacker_cost >= 999 * victim_loss);
    }

    #[test]
    fn assets_for_shares_prices_dust_to_zero_without_error() {
        // Same dust input that makes assets_for_redeem fail: 1 share of a
        // 1000-share pot worth 100 atoms floors to zero.
        assert_eq!(assets_for_shares(1, 100, 1_000, 9), Ok(0));
        assert_eq!(
            assets_for_redeem(1, 100, 1_000, 9),
            Err(RoshiError::ZeroOutput)
        );
        // The guards stay identical otherwise.
        assert_eq!(assets_for_shares(100_000, 1_000, 1_000_000, 6), Ok(100));
        assert_eq!(
            assets_for_shares(101, 100, 100, 9),
            Err(RoshiError::InvalidVaultState)
        );
    }

    #[test]
    fn redeem_assets_are_floor_rounded_and_cannot_overpay() {
        // Par: exact inverse of the deposit example.
        assert_eq!(assets_for_redeem(100_000, 1_000, 1_000_000, 6), Ok(100));
        assert_eq!(
            assets_for_redeem(1, 100, 1_000, 9),
            Err(RoshiError::ZeroOutput)
        );
        assert_eq!(
            assets_for_redeem(101, 100, 100, 9),
            Err(RoshiError::InvalidVaultState)
        );
    }

    #[test]
    fn full_redeem_returns_all_assets_up_to_virtual_dust() {
        // At or below par price the virtual position rounds away and a full
        // redeem drains the vault exactly.
        assert_eq!(
            assets_for_redeem(1_000_000_000, 1_000_000, 1_000_000_000, 6),
            Ok(1_000_000)
        );
        assert_eq!(assets_for_redeem(u64::MAX, 123, u64::MAX, 9), Ok(123));
        // Far above par (only reachable at dust-sized supplies) the virtual
        // position keeps its pro-rata slice as dust.
        assert_eq!(assets_for_redeem(10, 1_000, 10, 9), Ok(910));
    }

    #[test]
    fn deposit_redeem_round_trip_does_not_overpay() {
        let shares = shares_for_deposit(1, 3, 10, 9).unwrap();
        assert_eq!(shares, 2);
        assert_eq!(
            assets_for_redeem(shares, 3, 10, 9),
            Err(RoshiError::ZeroOutput)
        );

        let shares = shares_for_deposit(100, 333, 1_000, 9).unwrap();
        let assets = assets_for_redeem(shares, 333, 1_000, 9).unwrap();
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
            total_assets in 0u64..=1_000_000_000,
            total_shares in 0u64..=1_000_000_000,
            base_atoms in 1u64..=1_000_000_000,
            extra_atoms in 0u64..=1_000_000_000,
            base_decimals in 0u8..=9,
        ) {
            let larger_base_atoms = base_atoms.saturating_add(extra_atoms);
            let smaller = shares_for_deposit(base_atoms, total_assets, total_shares, base_decimals);
            let larger =
                shares_for_deposit(larger_base_atoms, total_assets, total_shares, base_decimals);

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
            base_decimals in 0u8..=9,
        ) {
            let smaller_shares = 1 + (share_seed % total_shares);
            let remaining = total_shares - smaller_shares;
            let larger_shares = smaller_shares + (extra_seed % (remaining + 1));

            let smaller =
                assets_for_redeem(smaller_shares, total_assets, total_shares, base_decimals);
            let larger =
                assets_for_redeem(larger_shares, total_assets, total_shares, base_decimals);

            if let (Ok(smaller), Ok(larger)) = (smaller, larger) {
                prop_assert!(larger >= smaller);
            }
        }

        #[test]
        fn prop_deposit_then_redeem_never_overpays(
            base_atoms in 1u64..=1_000_000_000,
            total_assets in 0u64..=1_000_000_000,
            total_shares in 0u64..=1_000_000_000,
            base_decimals in 0u8..=9,
        ) {
            if let Ok(shares) = shares_for_deposit(base_atoms, total_assets, total_shares, base_decimals) {
                // Redeem against the post-deposit state the mint produced.
                let total_assets = total_assets + base_atoms;
                let total_shares = total_shares + shares;
                if let Ok(assets) = assets_for_redeem(shares, total_assets, total_shares, base_decimals) {
                    prop_assert!(assets <= base_atoms);
                }
            }
        }

        #[test]
        fn prop_redeem_never_pays_more_than_total_assets(
            total_assets in 0u64..=u64::MAX,
            total_shares in 1u64..=u64::MAX,
            share_seed in any::<u64>(),
            base_decimals in 0u8..=9,
        ) {
            let shares = 1 + (share_seed % total_shares);
            match assets_for_redeem(shares, total_assets, total_shares, base_decimals) {
                Ok(assets) => prop_assert!(assets <= total_assets),
                Err(error) => prop_assert_eq!(error, RoshiError::ZeroOutput),
            }
        }

        #[test]
        fn prop_par_vault_pricing_is_exact(
            pot in 1u64..=1_000_000_000,
            base_atoms in 1u64..=1_000_000_000,
            share_seed in any::<u64>(),
            base_decimals in 0u8..=9,
        ) {
            // At par the virtual offset cancels: deposits mint at exactly the
            // empty-vault ratio and redeems pay exactly floor(shares / ratio).
            let ratio = u64::try_from(virtual_share_offset(base_decimals).unwrap()).unwrap();
            let supply = pot * ratio;
            prop_assert_eq!(
                shares_for_deposit(base_atoms, pot, supply, base_decimals),
                Ok(base_atoms * ratio)
            );

            let shares = 1 + (share_seed % supply);
            let redeemed = assets_for_redeem(shares, pot, supply, base_decimals);
            if shares / ratio == 0 {
                prop_assert_eq!(redeemed, Err(RoshiError::ZeroOutput));
            } else {
                prop_assert_eq!(redeemed, Ok(shares / ratio));
            }
        }

        #[test]
        fn prop_full_redeem_at_or_below_par_returns_total_assets(
            total_assets in 1u64..=1_000_000_000,
            excess_shares in 0u64..=1_000_000_000,
            base_decimals in 0u8..=9,
        ) {
            let ratio = u64::try_from(virtual_share_offset(base_decimals).unwrap()).unwrap();
            let total_shares = total_assets * ratio + excess_shares;
            prop_assert_eq!(
                assets_for_redeem(total_shares, total_assets, total_shares, base_decimals),
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
