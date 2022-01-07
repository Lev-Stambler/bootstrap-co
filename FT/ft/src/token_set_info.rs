use near_contract_standards::fungible_token::FungibleToken;
use near_internal_balance::ft::FungibleTokenBalances;
use near_sdk::{collections::Vector, env, AccountId, Balance};
use primitive_types::U256;

use crate::{FeeReceiver, SetInfo, TokenWithRatio};

const FEE_DENOMINATOR: u128 = 1_000_000_000_000_000;

impl SetInfo {
    pub(crate) fn new(set_ratios: Vec<TokenWithRatio>, set_initial_fee: FeeReceiver) -> Self {
        let mut ratios = Vector::new(b"set-ratio".to_vec());
        for ratio in set_ratios {
            ratios.push(&ratio);
        }
        if set_initial_fee.owner_fee > FEE_DENOMINATOR
            || set_initial_fee.platform_fee > FEE_DENOMINATOR
        {
            panic!("Expected the fees to be less than the fee denominator of {}", FEE_DENOMINATOR);
        }
        if set_initial_fee.owner_fee + set_initial_fee.platform_fee > FEE_DENOMINATOR {
            panic!(
                "Expected the sum of fees to be less than the fee denominator of {}",
                FEE_DENOMINATOR
            );
        }
        Self { ratios, fee: set_initial_fee }
    }

    pub(crate) fn on_burn(
        &self,
        balances: &mut FungibleTokenBalances,
        account_id: AccountId,
        amount: Balance,
    ) {
        for i in 0..self.ratios.len() {
            let ratio = &self.ratios.get(i).unwrap();
            balances.increase_balance(&account_id, &ratio.token_id, ratio.ratio as u128 * amount);
        }
    }

    pub(crate) fn change_owner_fee(&mut self, new_fee: u128) {
        let caller = env::predecessor_account_id();
        self.fee.owner_fee = new_fee;
    }

    /// Decrease the balances of the underlying tokens and wrap the tokens.
    /// Also, send the apportioned fee amount
    ///
    /// return the amount wrapped and given to the wrapper
    pub(crate) fn wrap(
        &self,
        owner: &AccountId,
        ft: &mut FungibleToken,
        balances: &mut FungibleTokenBalances,
        amount: Option<Balance>,
    ) -> Balance {
        // TODO: hmmmmm... should this be the predecessor or the signer???
        let caller = env::predecessor_account_id();
        let max_amount_wrapped = self.get_max_amount(balances, &caller);
        let amount_wrap = amount.unwrap_or(max_amount_wrapped);
        // TODO: add test for this
        if amount_wrap > max_amount_wrapped {
            panic!(
                "Maximum amount that can be wrapped is {}, tried wrapping {}",
                max_amount_wrapped, amount_wrap
            );
        }
        let owner_inrcr = (U256::from(amount_wrap) * U256::from(self.fee.owner_fee)
            / U256::from(FEE_DENOMINATOR))
        .as_u128();
        let platform_incr = (U256::from(amount_wrap) * U256::from(self.fee.platform_fee)
            / U256::from(FEE_DENOMINATOR))
        .as_u128();

        let amount_wrap_caller = amount_wrap - owner_inrcr - platform_incr;

        // Do the internal deposits
        ft.internal_deposit(&caller, amount_wrap_caller);
        ft.internal_deposit(&owner, owner_inrcr);
        ft.internal_deposit(&self.fee.platform_id, platform_incr);

        self.decrease_potentials(balances, amount_wrap, &caller);

        amount_wrap
    }

    fn decrease_potentials(
        &self,
        balances: &mut FungibleTokenBalances,
        amount_out: Balance,
        account_id: &AccountId,
    ) {
        for i in 0..self.ratios.len() {
            let ratio = &self.ratios.get(i).unwrap();
            balances.subtract_balance(
                &account_id,
                &ratio.token_id,
                (ratio.ratio as u128) * amount_out,
            )
        }
    }

    fn get_max_amount(&self, balances: &FungibleTokenBalances, account_id: &AccountId) -> Balance {
        let mut min = u128::MAX;
        for i in 0..self.ratios.len() {
            let ratio = &self.ratios.get(i).unwrap();
            let bal = balances.get_ft_balance(&account_id, &ratio.token_id);

            let amount_out = bal / (ratio.ratio as u128);
            if amount_out < min {
                min = amount_out;
            }
        }
        min
    }
}
