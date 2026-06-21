//! `Vault` account wire type and decode helpers.

use solana_program_error::{ProgramError, ProgramResult};
use solana_pubkey::Pubkey;
use wincode::{deserialize, SchemaRead, SchemaWrite};

use crate::{
    access::verify_access_merkle_proof,
    error::RoshiError,
    math::{
        checked_u64, mul_div_floor, share_price_from_assets, validate_percentage_bps,
        BPS_DENOMINATOR, SHARE_DECIMALS,
    },
    oracle::OracleConfig,
    state::VAULT_ACCOUNT_TAG,
    ID,
};

const FLAG_FALSE: u8 = 0;
const FLAG_TRUE: u8 = 1;

const fn flag(value: bool) -> u8 {
    value as u8
}

fn bool_flag(flag: u8) -> Result<bool, ProgramError> {
    match flag {
        FLAG_FALSE => Ok(false),
        FLAG_TRUE => Ok(true),
        _ => Err(RoshiError::InvalidVaultState.into()),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Admin,
    Strategist,
    NavAuthority,
    WithdrawalAuthority,
}

/// Admin-configured economic risk controls. Zero disables a control, so the
/// all-zeros default is "every control off".
#[derive(
    Clone, Copy, Debug, Default, Eq, PartialEq, codama_macros::CodamaType, SchemaWrite, SchemaRead,
)]
#[wincode(assert_zero_copy)]
#[repr(C)]
pub struct VaultControls {
    /// Clamp on the adaptive profit-unlock window: a reported gain drips over
    /// `min(now - last_update_ts, max_unlock_duration_secs)`. 0 = gains apply
    /// instantly (no smoothing).
    pub max_unlock_duration_secs: u32,
    /// Atomic redeems reject once the last NAV report is older than this
    /// (pre-first-report vaults are exempt). Deposits and queued redeems are
    /// never staleness-gated. 0 = disabled.
    pub max_report_age_secs: u32,
    /// Reports arriving sooner than this after the previous report are
    /// rejected (the first report is exempt). 0 = disabled.
    pub min_report_interval_secs: u32,
    /// Strike-eligible unstruck withdrawal tickets become cancellable again
    /// once `clock.slot >= request_slot + cancel_grace_slots` — the
    /// withdrawal-authority liveness escape. 0 = escape disabled.
    pub cancel_grace_slots: u32,
    /// A report may not move the net share price up by more than this many
    /// bps vs. the stored pre-report price. May exceed 10_000 (a bound above
    /// +100% is meaningful). 0 = disabled.
    pub max_nav_gain_bps: u16,
    /// Fee on atomic redemptions, retained by the pool for remaining
    /// holders. At most 10_000.
    pub atomic_redeem_fee_bps: u16,
    /// Oracle-valued swap output must be at least input value times
    /// `1 - max_swap_slippage_bps`. At most 10_000. 0 = disabled.
    pub max_swap_slippage_bps: u16,
    _padding: [u8; 2],
}

impl VaultControls {
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        max_unlock_duration_secs: u32,
        max_report_age_secs: u32,
        min_report_interval_secs: u32,
        cancel_grace_slots: u32,
        max_nav_gain_bps: u16,
        atomic_redeem_fee_bps: u16,
        max_swap_slippage_bps: u16,
    ) -> Self {
        Self {
            max_unlock_duration_secs,
            max_report_age_secs,
            min_report_interval_secs,
            cancel_grace_slots,
            max_nav_gain_bps,
            atomic_redeem_fee_bps,
            max_swap_slippage_bps,
            _padding: [0; 2],
        }
    }

    pub fn validate(&self) -> ProgramResult {
        // max_nav_gain_bps is deliberately not percentage-bounded: a gain
        // bound above +100% is meaningful.
        validate_percentage_bps(self.atomic_redeem_fee_bps)?;
        validate_percentage_bps(self.max_swap_slippage_bps)?;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, SchemaWrite, SchemaRead)]
#[wincode(assert_zero_copy)]
#[repr(C)]
pub struct Vault {
    pub base_oracle: OracleConfig,
    pub total_assets: u64,
    pub external_assets: u64,
    pub pending_withdrawal_assets: u64,
    pub fees_payable: u64,
    pub high_watermark: u64,
    pub report_epoch: u64,
    pub requested_withdrawal_shares: u64,
    pub last_update_ts: i64,
    /// Reported profit still locked from past gain reports; drips out
    /// linearly between the unlock timestamps. Always at most `total_assets`.
    pub locked_profit: u64,
    pub profit_unlock_start_ts: i64,
    pub profit_unlock_end_ts: i64,
    pub tag: [u8; 32],
    pub admin: [u8; 32],
    pub strategist: [u8; 32],
    pub nav_authority: [u8; 32],
    pub withdrawal_authority: [u8; 32],
    pub base_mint: [u8; 32],
    pub share_mint: [u8; 32],
    pub treasury: [u8; 32],
    pub last_report_hash: [u8; 32],
    pub access_merkle_root: [u8; 32],
    pub controls: VaultControls,
    pub performance_fee_bps: u16,
    pub withdrawal_buffer_bps: u16,
    pub tag_len: u8,
    pub base_decimals: u8,
    pub deposit_sub_account: u8,
    pub withdraw_sub_account: u8,
    deposits_paused_flag: u8,
    withdrawals_paused_flag: u8,
    manage_paused_flag: u8,
    private_flag: u8,
    external_enabled_flag: u8,
    pub bump: u8,
    _padding: [u8; 2],
}

impl Vault {
    pub const SEED: &'static [u8] = b"vault";
    pub const MAX_TAG_LEN: usize = 32;
    pub const SPACE: usize = std::mem::size_of::<Self>() + 1;

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        tag: &[u8],
        admin: [u8; 32],
        strategist: [u8; 32],
        nav_authority: [u8; 32],
        withdrawal_authority: [u8; 32],
        base_mint: [u8; 32],
        share_mint: [u8; 32],
        base_decimals: u8,
        base_oracle: OracleConfig,
        deposit_sub_account: u8,
        withdraw_sub_account: u8,
        treasury: [u8; 32],
        performance_fee_bps: u16,
        withdrawal_buffer_bps: u16,
        controls: VaultControls,
        private: bool,
        access_merkle_root: [u8; 32],
        bump: u8,
    ) -> Result<Self, ProgramError> {
        Self::validate_config(
            base_mint,
            share_mint,
            base_decimals,
            performance_fee_bps,
            withdrawal_buffer_bps,
        )?;
        controls.validate()?;
        base_oracle
            .validate()
            .map_err(|_| ProgramError::from(RoshiError::InvalidVaultState))?;

        let (tag, tag_len) = Self::pack_tag(tag)?;

        Ok(Self {
            base_oracle,
            total_assets: 0,
            external_assets: 0,
            pending_withdrawal_assets: 0,
            fees_payable: 0,
            high_watermark: 0,
            report_epoch: 0,
            requested_withdrawal_shares: 0,
            last_update_ts: 0,
            locked_profit: 0,
            profit_unlock_start_ts: 0,
            profit_unlock_end_ts: 0,
            tag,
            admin,
            strategist,
            nav_authority,
            withdrawal_authority,
            base_mint,
            share_mint,
            treasury,
            last_report_hash: [0; 32],
            access_merkle_root,
            controls,
            performance_fee_bps,
            withdrawal_buffer_bps,
            tag_len,
            base_decimals,
            deposit_sub_account,
            withdraw_sub_account,
            deposits_paused_flag: flag(false),
            withdrawals_paused_flag: flag(false),
            manage_paused_flag: flag(false),
            private_flag: flag(private),
            external_enabled_flag: flag(false),
            bump,
            _padding: [0; 2],
        })
    }

    /// Decode a `Vault` from raw Roshi account data — the wincode `Account::Vault`
    /// payload (a one-byte tag then the vault).
    pub fn from_account_data(data: &[u8]) -> Result<Self, ProgramError> {
        let (&tag, rest) = data
            .split_first()
            .ok_or(ProgramError::from(RoshiError::InvalidVaultAccount))?;
        if tag != VAULT_ACCOUNT_TAG {
            return Err(RoshiError::InvalidVaultAccount.into());
        }
        let vault: Self =
            deserialize(rest).map_err(|_| ProgramError::from(RoshiError::InvalidVaultAccount))?;
        vault.validate_state()?;
        Ok(vault)
    }

    pub fn validate_config(
        base_mint: [u8; 32],
        share_mint: [u8; 32],
        base_decimals: u8,
        performance_fee_bps: u16,
        withdrawal_buffer_bps: u16,
    ) -> ProgramResult {
        validate_percentage_bps(performance_fee_bps)?;
        validate_percentage_bps(withdrawal_buffer_bps)?;

        if base_mint == share_mint {
            return Err(ProgramError::InvalidArgument);
        }

        // Deposit/redeem pricing offsets virtual shares by
        // 10^(SHARE_DECIMALS - base_decimals); a base mint with more decimals
        // than the share mint has no valid offset.
        if base_decimals > SHARE_DECIMALS {
            return Err(RoshiError::InvalidDecimals.into());
        }

        Ok(())
    }

    pub fn pack_tag(tag: &[u8]) -> Result<([u8; Self::MAX_TAG_LEN], u8), ProgramError> {
        Self::validate_tag(tag)?;

        let mut packed_tag = [0; Self::MAX_TAG_LEN];
        packed_tag[..tag.len()].copy_from_slice(tag);

        Ok((packed_tag, tag.len() as u8))
    }

    pub fn unpack_tag(tag: &[u8; Self::MAX_TAG_LEN], tag_len: u8) -> Result<&[u8], ProgramError> {
        let tag_len = usize::from(tag_len);
        let tag = tag
            .get(..tag_len)
            .ok_or(ProgramError::from(RoshiError::InvalidVaultTag))?;
        Self::validate_tag(tag)?;

        Ok(tag)
    }

    pub fn tag_seed(&self) -> Result<&[u8], ProgramError> {
        Self::unpack_tag(&self.tag, self.tag_len)
    }

    pub fn find_address(tag: &[u8], base_mint: &Pubkey) -> Result<(Pubkey, u8), ProgramError> {
        Self::validate_tag(tag)?;

        Ok(Pubkey::find_program_address(
            &[Self::SEED, tag, base_mint.as_ref()],
            &ID,
        ))
    }

    fn validate_tag(tag: &[u8]) -> ProgramResult {
        if tag.is_empty() || tag.len() > Self::MAX_TAG_LEN {
            return Err(RoshiError::InvalidVaultTag.into());
        }

        Ok(())
    }

    pub fn authority_for_role(&self, role: Role) -> Pubkey {
        match role {
            Role::Admin => Pubkey::from(self.admin),
            Role::Strategist => Pubkey::from(self.strategist),
            Role::NavAuthority => Pubkey::from(self.nav_authority),
            Role::WithdrawalAuthority => Pubkey::from(self.withdrawal_authority),
        }
    }

    pub fn has_role(&self, role: Role, signer: &Pubkey) -> bool {
        self.authority_for_role(role) == *signer
    }

    /// Verify `vault_key` is the canonical PDA for this vault's tag and base mint.
    pub fn verify_address(&self, vault_key: &Pubkey) -> ProgramResult {
        let base_mint = Pubkey::from(self.base_mint);
        let (expected_vault_key, expected_bump) = Self::find_address(self.tag_seed()?, &base_mint)?;

        if vault_key != &expected_vault_key || self.bump != expected_bump {
            return Err(ProgramError::InvalidSeeds);
        }

        Ok(())
    }

    /// The economic share supply: circulating shares plus the shares already
    /// burned for in-flight withdrawals.
    pub fn economic_share_supply(&self, active_share_supply: u64) -> Result<u64, ProgramError> {
        active_share_supply
            .checked_add(self.requested_withdrawal_shares)
            .ok_or(ProgramError::from(RoshiError::Overflow))
    }

    /// Reported profit still locked at `now`: the full `locked_profit` until
    /// the window starts, decaying linearly to zero at the window end.
    pub fn remaining_locked_profit(&self, now: i64) -> Result<u64, ProgramError> {
        if now >= self.profit_unlock_end_ts {
            return Ok(0);
        }
        if now <= self.profit_unlock_start_ts {
            return Ok(self.locked_profit);
        }

        // start < now < end here, so both spans are positive.
        let window = self
            .profit_unlock_end_ts
            .checked_sub(self.profit_unlock_start_ts)
            .and_then(|span| u128::try_from(span).ok())
            .ok_or(ProgramError::from(RoshiError::Overflow))?;
        let left = self
            .profit_unlock_end_ts
            .checked_sub(now)
            .and_then(|span| u128::try_from(span).ok())
            .ok_or(ProgramError::from(RoshiError::Overflow))?;

        let remaining = mul_div_floor(u128::from(self.locked_profit), left, window)?;
        Ok(checked_u64(remaining)?)
    }

    /// Share-pricing NAV at `now`: `total_assets` minus the profit still
    /// dripping. Every pricing read (deposit mint, redeem dust guard, ticket
    /// strike, atomic-redeem entitlement) uses this, never raw `total_assets`.
    pub fn effective_total_assets(&self, now: i64) -> Result<u64, ProgramError> {
        let remaining = self.remaining_locked_profit(now)?;
        self.total_assets
            .checked_sub(remaining)
            .ok_or(ProgramError::from(RoshiError::InvalidVaultState))
    }

    /// Pay `amount` (priced at [`Self::effective_total_assets`]) out of the
    /// vault at `now`. Re-anchors the unlock window at `now` with the
    /// still-locked remainder — the unlock line is preserved up to one atom
    /// of floor rounding, and `amount <= effective` keeps the static
    /// `locked_profit <= total_assets` invariant true after the debit.
    pub fn debit_assets_at_effective(&mut self, amount: u64, now: i64) -> ProgramResult {
        let remaining = self.remaining_locked_profit(now)?;
        let effective = self
            .total_assets
            .checked_sub(remaining)
            .ok_or(ProgramError::from(RoshiError::InvalidVaultState))?;
        if amount > effective {
            return Err(RoshiError::InvalidVaultState.into());
        }

        self.total_assets = self
            .total_assets
            .checked_sub(amount)
            .ok_or(ProgramError::from(RoshiError::Overflow))?;
        self.locked_profit = remaining;
        if remaining > 0 {
            // remaining > 0 implies now < end, and the clock is monotone past
            // the recorded start, so start <= now < end holds.
            self.profit_unlock_start_ts = now;
        }
        Ok(())
    }

    /// Recognize a report's post-fee NAV at `now`. A gain re-locks in full —
    /// rolling any unfinished drip forward — over the span it was earned in,
    /// clamped to `controls.max_unlock_duration_secs`. A loss recognizes
    /// instantly: the locked remainder absorbs it first (`locked_profit = 0`
    /// with the lower `total_assets` means effective NAV never jumps up).
    pub fn apply_reported_nav(&mut self, net_total_assets: u64, now: i64) -> ProgramResult {
        let prior_effective = self.effective_total_assets(now)?;

        if net_total_assets > prior_effective {
            let gain = net_total_assets
                .checked_sub(prior_effective)
                .ok_or(ProgramError::from(RoshiError::Overflow))?;
            // Adaptive window: gains unlock over the span they were earned
            // in. No min clamp — rapid reports carry small gains.
            let elapsed = now.saturating_sub(self.last_update_ts).max(0);
            let window = elapsed.min(i64::from(self.controls.max_unlock_duration_secs));
            if window == 0 {
                self.locked_profit = 0;
            } else {
                self.locked_profit = gain;
            }
            self.profit_unlock_start_ts = now;
            self.profit_unlock_end_ts = now
                .checked_add(window)
                .ok_or(ProgramError::from(RoshiError::Overflow))?;
        } else {
            self.locked_profit = 0;
            self.profit_unlock_start_ts = now;
            self.profit_unlock_end_ts = now;
        }

        self.total_assets = net_total_assets;
        Ok(())
    }

    /// Staleness gate: reject when the last report is older than
    /// `controls.max_report_age_secs`. Applied to atomic redeems only —
    /// deposits are never staleness-gated (stale-entry capture is bounded by
    /// the drip and the gain bound; stale-high entry harms only the
    /// depositor) and queued redeems price later at strike. Pre-first-report
    /// vaults are exempt (pricing is exactly par via the virtual offset).
    pub fn verify_report_fresh(&self, now: i64) -> ProgramResult {
        let max_age = i64::from(self.controls.max_report_age_secs);
        if self.report_epoch == 0 || max_age == 0 {
            return Ok(());
        }
        if now.saturating_sub(self.last_update_ts) > max_age {
            return Err(RoshiError::StaleNavReport.into());
        }
        Ok(())
    }

    /// Report rate limit: reject reports arriving sooner than
    /// `controls.min_report_interval_secs` after the previous one (the first
    /// report is exempt). Without this, a compromised NAV authority chains
    /// small in-bound reports past the gain bound.
    pub fn verify_report_interval(&self, now: i64) -> ProgramResult {
        let interval = i64::from(self.controls.min_report_interval_secs);
        if self.report_epoch == 0 || interval == 0 {
            return Ok(());
        }
        if now.saturating_sub(self.last_update_ts) < interval {
            return Err(RoshiError::ReportTooFrequent.into());
        }
        Ok(())
    }

    /// NAV gain bound: a report may not raise the net share price by more
    /// than `controls.max_nav_gain_bps` vs. the stored pre-report price. No
    /// downward bound — honest losses must land in one report. An over-bound
    /// honest gain is not lost: the authority reports the capped amount and
    /// rolls the remainder into subsequent reports. Skipped when supply or
    /// the stored price is zero so post-total-loss recovery cannot wedge.
    pub fn verify_nav_gain_bound(
        &self,
        net_total_assets: u64,
        economic_share_supply: u64,
    ) -> ProgramResult {
        if self.controls.max_nav_gain_bps == 0 || economic_share_supply == 0 {
            return Ok(());
        }
        let pre_price = share_price_from_assets(self.total_assets, economic_share_supply)?;
        if pre_price == 0 {
            return Ok(());
        }

        let new_price = share_price_from_assets(net_total_assets, economic_share_supply)?;
        let max_price = checked_u64(mul_div_floor(
            u128::from(pre_price),
            u128::from(BPS_DENOMINATOR) + u128::from(self.controls.max_nav_gain_bps),
            u128::from(BPS_DENOMINATOR),
        )?)?;
        if new_price > max_price {
            return Err(RoshiError::NavGainExceedsBound.into());
        }
        Ok(())
    }

    /// Base custody only ever moves through the sub-accounts whose base ATAs
    /// `report_nav` reads as idle — the vault's current deposit and withdraw
    /// sub-accounts. External investment, returns, and fee collection are pinned
    /// to these so the on-chain idle read always covers base in the *current*
    /// custodies. The admin may repoint either sub-account, but every base
    /// movement stays consistent with whatever the vault currently designates.
    ///
    /// Repointing while the old custody still holds base strands it: the on-chain
    /// idle read no longer sees it, so the off-chain NAV must fold that balance
    /// into the reported `external_value`.
    pub fn verify_idle_sub_account(&self, sub_account: u8) -> ProgramResult {
        if sub_account == self.deposit_sub_account || sub_account == self.withdraw_sub_account {
            return Ok(());
        }

        Err(RoshiError::InvalidSubAccount.into())
    }

    pub fn verify_manage_enabled(&self) -> ProgramResult {
        if self.manage_paused()? {
            return Err(RoshiError::VaultPaused.into());
        }

        Ok(())
    }

    pub fn allows_depositor(&self, depositor: &Pubkey, proof: &[[u8; 32]]) -> bool {
        match self.private() {
            Ok(false) => true,
            Ok(true) => verify_access_merkle_proof(depositor, &self.access_merkle_root, proof),
            Err(_) => false,
        }
    }

    pub fn deposits_paused(&self) -> Result<bool, ProgramError> {
        bool_flag(self.deposits_paused_flag)
    }

    pub fn withdrawals_paused(&self) -> Result<bool, ProgramError> {
        bool_flag(self.withdrawals_paused_flag)
    }

    pub fn manage_paused(&self) -> Result<bool, ProgramError> {
        bool_flag(self.manage_paused_flag)
    }

    pub fn private(&self) -> Result<bool, ProgramError> {
        bool_flag(self.private_flag)
    }

    pub fn external_enabled(&self) -> Result<bool, ProgramError> {
        bool_flag(self.external_enabled_flag)
    }

    pub fn set_deposits_paused(&mut self, deposits_paused: bool) {
        self.deposits_paused_flag = flag(deposits_paused);
    }

    pub fn set_withdrawals_paused(&mut self, withdrawals_paused: bool) {
        self.withdrawals_paused_flag = flag(withdrawals_paused);
    }

    pub fn set_manage_paused(&mut self, manage_paused: bool) {
        self.manage_paused_flag = flag(manage_paused);
    }

    pub fn set_private(&mut self, private: bool) {
        self.private_flag = flag(private);
    }

    pub fn set_external_enabled(&mut self, external_enabled: bool) {
        self.external_enabled_flag = flag(external_enabled);
    }

    pub fn validate_state(&self) -> ProgramResult {
        Self::unpack_tag(&self.tag, self.tag_len)?;
        Self::validate_config(
            self.base_mint,
            self.share_mint,
            self.base_decimals,
            self.performance_fee_bps,
            self.withdrawal_buffer_bps,
        )?;
        self.base_oracle
            .validate()
            .map_err(|_| ProgramError::from(RoshiError::InvalidVaultState))?;
        self.controls.validate()?;
        if self.locked_profit > self.total_assets {
            return Err(RoshiError::InvalidVaultState.into());
        }
        if self.profit_unlock_start_ts > self.profit_unlock_end_ts {
            return Err(RoshiError::InvalidVaultState.into());
        }
        bool_flag(self.deposits_paused_flag)?;
        bool_flag(self.withdrawals_paused_flag)?;
        bool_flag(self.manage_paused_flag)?;
        bool_flag(self.private_flag)?;
        bool_flag(self.external_enabled_flag)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::access::{access_merkle_leaf, access_merkle_node};
    use wincode::{config::DefaultConfig, serialize, SchemaRead, SchemaWrite, TypeMeta};

    fn assert_zero_copy<T>()
    where
        T: wincode::ZeroCopy,
        T: for<'de> SchemaRead<'de, DefaultConfig> + SchemaWrite<DefaultConfig>,
    {
        assert_eq!(
            <T as SchemaRead<'_, DefaultConfig>>::TYPE_META,
            TypeMeta::Static {
                size: core::mem::size_of::<T>(),
                zero_copy: true,
            }
        );
        assert_eq!(
            <T as SchemaWrite<DefaultConfig>>::TYPE_META,
            TypeMeta::Static {
                size: core::mem::size_of::<T>(),
                zero_copy: true,
            }
        );
    }

    pub(crate) fn new_test_vault(private: bool, access_merkle_root: [u8; 32]) -> Vault {
        let admin = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let (_, bump) = Vault::find_address(b"test", &base_mint).unwrap();

        Vault::new(
            b"test",
            admin.to_bytes(),
            [2; 32],
            [4; 32],
            [5; 32],
            base_mint.to_bytes(),
            Pubkey::new_unique().to_bytes(),
            6,
            OracleConfig::default(),
            7,
            8,
            [9; 32],
            100,
            250,
            VaultControls::default(),
            private,
            access_merkle_root,
            bump,
        )
        .unwrap()
    }

    #[test]
    fn new_initializes_default_accounting_and_config() {
        let vault = new_test_vault(true, [10; 32]);

        assert_eq!(vault.tag_seed().unwrap(), b"test");
        assert_eq!(vault.strategist, [2; 32]);
        assert_eq!(vault.nav_authority, [4; 32]);
        assert_eq!(vault.withdrawal_authority, [5; 32]);
        assert_eq!(vault.base_decimals, 6);
        assert_eq!(vault.deposit_sub_account, 7);
        assert_eq!(vault.withdraw_sub_account, 8);
        assert_eq!(vault.treasury, [9; 32]);
        assert_eq!(vault.total_assets, 0);
        assert_eq!(vault.external_assets, 0);
        assert_eq!(vault.pending_withdrawal_assets, 0);
        assert_eq!(vault.fees_payable, 0);
        assert_eq!(vault.high_watermark, 0);
        assert_eq!(vault.report_epoch, 0);
        assert_eq!(vault.requested_withdrawal_shares, 0);
        assert_eq!(vault.locked_profit, 0);
        assert_eq!(vault.profit_unlock_start_ts, 0);
        assert_eq!(vault.profit_unlock_end_ts, 0);
        assert_eq!(vault.performance_fee_bps, 100);
        assert_eq!(vault.withdrawal_buffer_bps, 250);
        assert_eq!(vault.controls, VaultControls::default());
        assert_eq!(vault.last_update_ts, 0);
        assert_eq!(vault.deposits_paused(), Ok(false));
        assert_eq!(vault.withdrawals_paused(), Ok(false));
        assert_eq!(vault.manage_paused(), Ok(false));
        assert_eq!(vault.private(), Ok(true));
        assert_eq!(vault.external_enabled(), Ok(false));
        assert_eq!(vault.access_merkle_root, [10; 32]);
    }

    #[test]
    fn from_account_data_round_trips_a_tagged_vault() {
        let vault = new_test_vault(false, [0; 32]);
        let mut data = vec![VAULT_ACCOUNT_TAG];
        data.extend_from_slice(&serialize(&vault).unwrap());

        assert_eq!(Vault::from_account_data(&data).unwrap(), vault);
    }

    #[test]
    fn from_account_data_rejects_wrong_tag() {
        let vault = new_test_vault(false, [0; 32]);
        let mut data = vec![VAULT_ACCOUNT_TAG + 1];
        data.extend_from_slice(&serialize(&vault).unwrap());

        assert_eq!(
            Vault::from_account_data(&data),
            Err(ProgramError::from(RoshiError::InvalidVaultAccount))
        );
    }

    #[test]
    fn vault_is_zero_copy_with_explicit_padding() {
        assert_zero_copy::<Vault>();
        assert_eq!(core::mem::size_of::<VaultControls>(), 24);
        assert_eq!(core::mem::size_of::<Vault>(), 648);
        assert_eq!(Vault::SPACE, 649);
        let vault = new_test_vault(false, [0; 32]);
        assert_eq!(
            serialize(&vault).unwrap().len(),
            core::mem::size_of::<Vault>()
        );
    }

    #[test]
    fn vault_controls_reject_invalid_percentage_bps() {
        assert!(VaultControls::new(0, 0, 0, 0, 0, 10_001, 0)
            .validate()
            .is_err());
        assert!(VaultControls::new(0, 0, 0, 0, 0, 0, 10_001)
            .validate()
            .is_err());
        // The gain bound is not a percentage; above-100% bounds are legal.
        assert!(VaultControls::new(0, 0, 0, 0, 60_000, 10_000, 10_000)
            .validate()
            .is_ok());
    }

    /// A vault mid-drip: `locked` profit unlocking linearly over
    /// `[start, end]` on top of `total` assets.
    fn drip_vault(total: u64, locked: u64, start: i64, end: i64) -> Vault {
        let mut vault = new_test_vault(false, [0; 32]);
        vault.total_assets = total;
        vault.locked_profit = locked;
        vault.profit_unlock_start_ts = start;
        vault.profit_unlock_end_ts = end;
        vault
    }

    #[test]
    fn remaining_locked_profit_interpolates_linearly() {
        let vault = drip_vault(2_000, 1_000, 0, 100);

        assert_eq!(vault.remaining_locked_profit(-5), Ok(1_000));
        assert_eq!(vault.remaining_locked_profit(0), Ok(1_000));
        assert_eq!(vault.remaining_locked_profit(25), Ok(750));
        assert_eq!(vault.remaining_locked_profit(50), Ok(500));
        assert_eq!(vault.remaining_locked_profit(99), Ok(10));
        assert_eq!(vault.remaining_locked_profit(100), Ok(0));
        assert_eq!(vault.remaining_locked_profit(1_000), Ok(0));

        assert_eq!(vault.effective_total_assets(50), Ok(1_500));
        assert_eq!(vault.effective_total_assets(100), Ok(2_000));
    }

    #[test]
    fn debit_at_effective_re_anchors_without_moving_the_unlock_line() {
        let mut vault = drip_vault(2_000, 1_000, 0, 100);
        let expected_remaining_at_70 = vault.remaining_locked_profit(70).unwrap();

        vault.debit_assets_at_effective(1_400, 40).unwrap();

        assert_eq!(vault.total_assets, 600);
        assert_eq!(vault.locked_profit, 600);
        assert_eq!(vault.profit_unlock_start_ts, 40);
        assert_eq!(vault.profit_unlock_end_ts, 100);
        assert_eq!(
            vault.remaining_locked_profit(70),
            Ok(expected_remaining_at_70)
        );
        assert!(vault.validate_state().is_ok());
    }

    #[test]
    fn debit_at_effective_rejects_amounts_above_effective() {
        let mut vault = drip_vault(2_000, 1_000, 0, 100);
        let before = vault;

        // Effective at t=40 is 1_400.
        assert_eq!(
            vault.debit_assets_at_effective(1_401, 40),
            Err(ProgramError::from(RoshiError::InvalidVaultState))
        );
        assert_eq!(vault, before);
    }

    #[test]
    fn apply_reported_nav_locks_gains_over_the_elapsed_window() {
        let mut vault = new_test_vault(false, [0; 32]);
        vault.controls = VaultControls::new(1_000, 0, 0, 0, 0, 0, 0);
        vault.total_assets = 1_000;
        vault.last_update_ts = 100;

        vault.apply_reported_nav(1_600, 400).unwrap();

        assert_eq!(vault.total_assets, 1_600);
        assert_eq!(vault.locked_profit, 600);
        assert_eq!(vault.profit_unlock_start_ts, 400);
        // Earned over 300s < 1_000s clamp: drips over the same 300s.
        assert_eq!(vault.profit_unlock_end_ts, 700);
        // Effective NAV is continuous through the report.
        assert_eq!(vault.effective_total_assets(400), Ok(1_000));
    }

    #[test]
    fn apply_reported_nav_clamps_the_window() {
        let mut vault = new_test_vault(false, [0; 32]);
        vault.controls = VaultControls::new(1_000, 0, 0, 0, 0, 0, 0);
        vault.total_assets = 1_000;
        vault.last_update_ts = 0;

        vault.apply_reported_nav(1_600, 5_000).unwrap();

        assert_eq!(vault.profit_unlock_start_ts, 5_000);
        assert_eq!(vault.profit_unlock_end_ts, 6_000);
    }

    #[test]
    fn apply_reported_nav_rolls_unfinished_drip_forward() {
        let mut vault = drip_vault(1_600, 600, 400, 700);
        vault.controls = VaultControls::new(1_000, 0, 0, 0, 0, 0, 0);
        vault.last_update_ts = 400;

        // Mid-drip at t=550: remaining 300, effective 1_300.
        vault.apply_reported_nav(1_700, 550).unwrap();

        assert_eq!(vault.total_assets, 1_700);
        // gain = 1_700 - 1_300: the unfinished 300 re-locks with the new 100.
        assert_eq!(vault.locked_profit, 400);
        assert_eq!(vault.profit_unlock_start_ts, 550);
        assert_eq!(vault.profit_unlock_end_ts, 700);
        assert_eq!(vault.effective_total_assets(550), Ok(1_300));
    }

    #[test]
    fn apply_reported_nav_applies_losses_instantly() {
        let mut vault = drip_vault(1_600, 600, 400, 700);
        vault.controls = VaultControls::new(1_000, 0, 0, 0, 0, 0, 0);
        vault.last_update_ts = 400;

        // Effective at t=550 is 1_300; reporting below it is a loss.
        vault.apply_reported_nav(1_200, 550).unwrap();

        assert_eq!(vault.total_assets, 1_200);
        assert_eq!(vault.locked_profit, 0);
        assert_eq!(vault.effective_total_assets(550), Ok(1_200));
    }

    #[test]
    fn apply_reported_nav_with_unlock_disabled_recognizes_gains_instantly() {
        let mut vault = new_test_vault(false, [0; 32]);
        vault.total_assets = 1_000;
        vault.last_update_ts = 100;

        vault.apply_reported_nav(1_600, 400).unwrap();

        assert_eq!(vault.locked_profit, 0);
        assert_eq!(vault.effective_total_assets(400), Ok(1_600));
    }

    #[test]
    fn verify_report_fresh_gates_only_configured_post_first_report_vaults() {
        let mut vault = new_test_vault(false, [0; 32]);
        vault.last_update_ts = 0;

        // Disabled control: never stale.
        assert!(vault.verify_report_fresh(i64::MAX).is_ok());

        vault.controls = VaultControls::new(0, 100, 0, 0, 0, 0, 0);
        // Pre-first-report vaults are exempt.
        assert!(vault.verify_report_fresh(1_000).is_ok());

        vault.report_epoch = 1;
        assert!(vault.verify_report_fresh(100).is_ok());
        assert_eq!(
            vault.verify_report_fresh(101),
            Err(ProgramError::from(RoshiError::StaleNavReport))
        );
    }

    #[test]
    fn verify_report_interval_rejects_rapid_reports() {
        let mut vault = new_test_vault(false, [0; 32]);
        vault.controls = VaultControls::new(0, 0, 60, 0, 0, 0, 0);
        vault.last_update_ts = 1_000;

        // The first report is exempt.
        assert!(vault.verify_report_interval(1_001).is_ok());

        vault.report_epoch = 1;
        assert_eq!(
            vault.verify_report_interval(1_059),
            Err(ProgramError::from(RoshiError::ReportTooFrequent))
        );
        assert!(vault.verify_report_interval(1_060).is_ok());
    }

    #[test]
    fn verify_nav_gain_bound_caps_upward_price_moves_only() {
        let mut vault = new_test_vault(false, [0; 32]);
        vault.controls = VaultControls::new(0, 0, 0, 0, 1_000, 0, 0);
        vault.total_assets = 1_000;
        let supply = 1_000_000_000;

        // +10% exactly passes; one atom more is rejected.
        assert!(vault.verify_nav_gain_bound(1_100, supply).is_ok());
        assert_eq!(
            vault.verify_nav_gain_bound(1_101, supply),
            Err(ProgramError::from(RoshiError::NavGainExceedsBound))
        );
        // No downward bound.
        assert!(vault.verify_nav_gain_bound(0, supply).is_ok());
        // Skips: supply zero, stored price zero, control disabled.
        assert!(vault.verify_nav_gain_bound(u64::MAX, 0).is_ok());
        vault.total_assets = 0;
        assert!(vault.verify_nav_gain_bound(u64::MAX, supply).is_ok());
        vault.total_assets = 1_000;
        vault.controls = VaultControls::default();
        assert!(vault.verify_nav_gain_bound(u64::MAX, supply).is_ok());
    }

    mod drip_properties {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(256))]

            /// Remaining locked profit never increases with time and never
            /// exceeds the locked amount.
            #[test]
            fn prop_remaining_locked_is_monotone_and_bounded(
                locked in 0u64..=1_000_000_000_000,
                extra in 0u64..=1_000_000_000_000,
                start in 0i64..=1_000_000_000,
                window in 0i64..=10_000_000,
                t1 in -1_000i64..=20_000_000,
                dt in 0i64..=20_000_000,
            ) {
                let vault = drip_vault(
                    locked.saturating_add(extra),
                    locked,
                    start,
                    start + window,
                );
                let early = vault.remaining_locked_profit(start + t1).unwrap();
                let late = vault.remaining_locked_profit(start + t1 + dt).unwrap();

                prop_assert!(early <= locked);
                prop_assert!(late <= early);
            }

            /// Debiting at effective NAV preserves the unlock line up to one
            /// atom of floor rounding, and keeps the state invariant valid.
            #[test]
            fn prop_debit_preserves_unlock_line_within_one_atom(
                locked in 0u64..=1_000_000_000_000,
                extra in 0u64..=1_000_000_000_000,
                start in 0i64..=1_000_000,
                window in 1i64..=10_000_000,
                now_offset in 0i64..=10_000_000,
                t_offset in 0i64..=10_000_000,
                amount_seed in any::<u64>(),
            ) {
                let total = locked.saturating_add(extra);
                let now = start + now_offset.min(window);
                let t = now + t_offset.min(window);
                let mut vault = drip_vault(locked.saturating_add(extra), locked, start, start + window);

                let before = vault.remaining_locked_profit(t).unwrap();
                let effective = vault.effective_total_assets(now).unwrap();
                let amount = if effective == 0 { 0 } else { amount_seed % (effective + 1) };

                vault.debit_assets_at_effective(amount, now).unwrap();

                let after = vault.remaining_locked_profit(t).unwrap();
                prop_assert!(after <= before);
                prop_assert!(before - after <= 1);
                prop_assert_eq!(vault.total_assets, total - amount);
                prop_assert!(vault.validate_state().is_ok());
            }
        }
    }

    #[test]
    fn validate_state_rejects_locked_profit_above_total_assets() {
        let mut vault = new_test_vault(false, [0; 32]);
        vault.total_assets = 100;
        vault.locked_profit = 101;

        assert_eq!(
            vault.validate_state(),
            Err(ProgramError::from(RoshiError::InvalidVaultState))
        );
    }

    #[test]
    fn validate_state_rejects_inverted_unlock_window() {
        let mut vault = new_test_vault(false, [0; 32]);
        vault.profit_unlock_start_ts = 10;
        vault.profit_unlock_end_ts = 9;

        assert_eq!(
            vault.validate_state(),
            Err(ProgramError::from(RoshiError::InvalidVaultState))
        );
    }

    #[test]
    fn pause_and_access_flags_use_typed_accessors() {
        let mut vault = new_test_vault(false, [0; 32]);

        assert_eq!(vault.deposits_paused(), Ok(false));
        assert_eq!(vault.withdrawals_paused(), Ok(false));
        assert_eq!(vault.manage_paused(), Ok(false));
        assert_eq!(vault.private(), Ok(false));
        assert_eq!(vault.external_enabled(), Ok(false));

        vault.set_deposits_paused(true);
        vault.set_withdrawals_paused(true);
        vault.set_manage_paused(true);
        vault.set_private(true);
        vault.set_external_enabled(true);

        assert_eq!(vault.deposits_paused(), Ok(true));
        assert_eq!(vault.withdrawals_paused(), Ok(true));
        assert_eq!(vault.manage_paused(), Ok(true));
        assert_eq!(vault.private(), Ok(true));
        assert_eq!(vault.external_enabled(), Ok(true));
    }

    #[test]
    fn verify_manage_enabled_rejects_paused_vault() {
        let mut vault = new_test_vault(false, [0; 32]);

        vault.set_manage_paused(true);

        assert_eq!(
            vault.verify_manage_enabled(),
            Err(ProgramError::from(RoshiError::VaultPaused))
        );
    }

    #[test]
    fn unpack_tag_rejects_invalid_tags() {
        let (tag, _) = Vault::pack_tag(b"test").unwrap();

        assert!(matches!(
            Vault::unpack_tag(&tag, 0),
            Err(error) if error == ProgramError::from(RoshiError::InvalidVaultTag)
        ));
        assert!(matches!(
            Vault::unpack_tag(&tag, 33),
            Err(error) if error == ProgramError::from(RoshiError::InvalidVaultTag)
        ));
    }

    #[test]
    fn validate_config_rejects_invalid_bps() {
        assert!(matches!(
            Vault::validate_config([1; 32], [2; 32], 6, 10_001, 0),
            Err(error) if error == ProgramError::from(RoshiError::InvalidBps)
        ));
    }

    #[test]
    fn validate_config_rejects_matching_base_and_share_mints() {
        assert!(matches!(
            Vault::validate_config([1; 32], [1; 32], 6, 0, 0),
            Err(ProgramError::InvalidArgument)
        ));
    }

    #[test]
    fn validate_config_rejects_base_decimals_above_share_decimals() {
        assert!(Vault::validate_config([1; 32], [2; 32], 9, 0, 0).is_ok());
        assert!(matches!(
            Vault::validate_config([1; 32], [2; 32], 10, 0, 0),
            Err(error) if error == ProgramError::from(RoshiError::InvalidDecimals)
        ));
    }

    #[test]
    fn from_account_data_rejects_invalid_vault_flags() {
        let mut vault = new_test_vault(false, [0; 32]);
        vault.manage_paused_flag = 255;
        let mut data = vec![VAULT_ACCOUNT_TAG];
        data.extend_from_slice(&serialize(&vault).unwrap());

        assert_eq!(
            Vault::from_account_data(&data),
            Err(ProgramError::from(RoshiError::InvalidVaultState))
        );
    }

    #[test]
    fn from_account_data_rejects_invalid_base_oracle_kind() {
        let vault = new_test_vault(false, [0; 32]);
        let mut data = vec![VAULT_ACCOUNT_TAG];
        data.extend_from_slice(&serialize(&vault).unwrap());
        let oracle_kind_offset = 1
            + core::mem::size_of::<crate::oracle::SwitchboardOracleConfig>()
            + core::mem::size_of::<crate::oracle::PythOracleConfig>();
        data[oracle_kind_offset] = 255;

        assert_eq!(
            Vault::from_account_data(&data),
            Err(ProgramError::from(RoshiError::InvalidVaultState))
        );
    }

    #[test]
    fn public_vault_allows_any_depositor_without_proof() {
        let vault = new_test_vault(false, [0; 32]);

        assert!(vault.allows_depositor(&Pubkey::new_unique(), &[]));
        assert!(vault.allows_depositor(&Pubkey::new_unique(), &[[7; 32]]));
    }

    #[test]
    fn private_vault_accepts_valid_access_proof() {
        let allowed = Pubkey::new_unique();
        let sibling = access_merkle_leaf(&Pubkey::new_unique());
        let root = access_merkle_node(&access_merkle_leaf(&allowed), &sibling);
        let vault = new_test_vault(true, root);

        assert!(vault.allows_depositor(&allowed, &[sibling]));
    }

    #[test]
    fn private_vault_rejects_missing_or_wrong_access_proof() {
        let allowed = Pubkey::new_unique();
        let sibling = access_merkle_leaf(&Pubkey::new_unique());
        let root = access_merkle_node(&access_merkle_leaf(&allowed), &sibling);
        let vault = new_test_vault(true, root);

        assert!(!vault.allows_depositor(&allowed, &[]));
        assert!(!vault.allows_depositor(&Pubkey::new_unique(), &[sibling]));
        assert!(!vault.allows_depositor(&allowed, &[[9; 32]]));
    }
}
