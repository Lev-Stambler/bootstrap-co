/*!
Fungible Token implementation with JSON serialization.
NOTES:
  - The maximum balance value is limited by U128 (2**128 - 1).
  - JSON calls should pass U128 as a base-10 string. E.g. "100".
  - The contract optimizes the inner trie structure by hashing account IDs. It will prevent some
    abuse of deep tries. Shouldn't be an issue, once NEAR clients implement full hashing of keys.
  - The contract tracks the change in storage before and after the call. If the storage increases,
    the contract requires the caller of the contract to attach enough deposit to the function call
    to cover the storage cost.
    This is done to prevent a denial of service attack on the contract by taking all available storage.
    If the storage decreases, the contract will issue a refund for the cost of the released storage.
    The unused tokens from the attached deposit are also refunded, so it's safe to
    attach more deposit than required.
  - To prevent the deployed contract from being modified or deleted, it should not have any access
    keys on its account.
*/
use near_contract_standards::fungible_token::metadata::{
    FungibleTokenMetadata, FungibleTokenMetadataProvider, FT_METADATA_SPEC,
};
use near_contract_standards::fungible_token::FungibleToken;
use near_internal_balance::ft::{FungibleTokenBalances, FungibleTokenHandlers};
use near_internal_balance::NearInternalBalance;
use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::{LazyOption, Vector};
use near_sdk::json_types::{ValidAccountId, U128};
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::{env, log, near_bindgen, AccountId, Balance, PanicOnDefault, PromiseOrValue};

mod token_set_info;
mod utils;

near_sdk::setup_alloc!();

// TODO: do we bake in hardcoded fee to us????
#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault, Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
/// Contains the fees for minting tokens.
pub struct FeeReceiver {
    /// The fee for the owner of the token set
    owner_fee: u128,
    /// The fee for the "platform" which created the token set contract
    platform_fee: u128,
    /// The platform account to receive te token
    platform_id: AccountId,
}

#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault, Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub struct TokenWithRatioValid {
    pub token_id: ValidAccountId,
    pub ratio: u32,
}

#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault, Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub struct TokenWithRatio {
    token_id: AccountId,
    ratio: u32,
}

#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault)]
pub struct SetInfo {
    ratios: Vector<TokenWithRatio>,
    fee: FeeReceiver,
}

#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault, NearInternalBalance)]
pub struct Contract {
    owner_id: AccountId,
    token: FungibleToken,
    metadata: LazyOption<FungibleTokenMetadata>,
    balances: FungibleTokenBalances,
    set_info: SetInfo,
}

#[near_bindgen]
impl Contract {
    /// Initializes the contract with the given total supply owned by the given `owner_id` with
    /// default metadata (for example purposes only).
    #[init]
    pub fn new_default_meta(
        owner_id: ValidAccountId,
        name: String,
        symbol: String,
        icon_url: Option<String>,
        set_ratios: Vec<TokenWithRatioValid>,
        // TODO: should be hardcoded?
        platform_fee: U128,
        platform_id: ValidAccountId,
        owner_fee: U128,
    ) -> Self {
        Self::new(
            owner_id,
            FungibleTokenMetadata {
                spec: FT_METADATA_SPEC.to_string(),
                name: name,
                symbol: symbol,
                icon: icon_url,
                reference: None,
                reference_hash: None,
                decimals: 24,
            },
            set_ratios,
            FeeReceiver {
                platform_fee: platform_fee.0,
                owner_fee: owner_fee.0,
                platform_id: platform_id.to_string(),
            },
        )
    }

    /// Initializes the contract with the given total supply owned by the given `owner_id` with
    /// the given fungible token metadata.
    #[init]
    pub fn new(
        owner_id: ValidAccountId,
        metadata: FungibleTokenMetadata,
        set_ratios: Vec<TokenWithRatioValid>,
        set_initial_fee: FeeReceiver,
    ) -> Self {
        assert!(!env::state_exists(), "Already initialized");

        metadata.assert_valid();
        let numb_tokens = set_ratios.len();
        let mut this = Self {
            owner_id: owner_id.to_string(),
            token: FungibleToken::new(b"a".to_vec()),
            metadata: LazyOption::new(b"m".to_vec(), Some(&metadata)),
            balances: FungibleTokenBalances::new(),
            set_info: SetInfo::new(set_ratios, set_initial_fee),
        };
        
        this
    }

    #[payable]
    pub fn wrap(&mut self, amount: Option<u128>) {
        utils::assert_1_yocto();
        self.set_info.wrap(&self.owner_id, &mut self.token, &mut self.balances, amount);
    }

    #[payable]
    pub fn update_owner_fee(&mut self, new_fee: u128) {
        utils::assert_1_yocto();
        assert_eq!(
            env::predecessor_account_id(),
            self.owner_id,
            "Only the owner can update the fee"
        );

        self.set_info.change_owner_fee(new_fee);
    }

    // TODO: let's think about,
    // if there account was deleted that means we have to do something with the balance
    // maybe we j transfer to platform?
    fn on_account_closed(&mut self, account_id: AccountId, balance: Balance) {
        log!("Closed @{} with {}", account_id, balance);
        let platform_id = self.set_info.fee.platform_id.clone();
        self.set_info.on_burn(&mut self.balances, platform_id, balance);
    }

    fn on_tokens_burned(&mut self, account_id: AccountId, amount: Balance) {
        log!("Account @{} burned {}", account_id, amount);
        self.set_info.on_burn(&mut self.balances, account_id, amount);
    }
}

near_contract_standards::impl_fungible_token_core!(Contract, token, on_tokens_burned);
near_contract_standards::impl_fungible_token_storage!(Contract, token, on_account_closed);

#[near_bindgen]
impl FungibleTokenMetadataProvider for Contract {
    fn ft_metadata(&self) -> FungibleTokenMetadata {
        self.metadata.get().unwrap()
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use near_sdk::test_utils::{accounts, VMContextBuilder};
    use near_sdk::MockedBlockchain;
    use near_sdk::{testing_env, Balance};

    use super::*;

    fn get_context(predecessor_account_id: ValidAccountId) -> VMContextBuilder {
        let mut builder = VMContextBuilder::new();
        builder
            .current_account_id(accounts(0))
            .signer_account_id(predecessor_account_id.clone())
            .predecessor_account_id(predecessor_account_id);
        builder
    }

    #[test]
    fn test_new() {
        let mut context = get_context(accounts(1));
        testing_env!(context.build());

        let platform_id = accounts(4);
        let token_id = accounts(5);

        let contract = Contract::new_default_meta(
            accounts(2).into(),
            "YOUR MOM".to_string(),
            "YOUR MOM".to_string(),
            None,
            vec![TokenWithRatioValid { token_id, ratio: 1 }],
            0.into(),
            platform_id,
            0.into(),
        );
        testing_env!(context.is_view(true).build());
        assert_eq!(contract.ft_total_supply().0, 0);
        assert_eq!(contract.ft_balance_of(accounts(1)).0, 0);
    }

    #[test]
    #[should_panic(expected = "The contract is not initialized")]
    fn test_default() {
        let context = get_context(accounts(1));
        testing_env!(context.build());
        let _contract = Contract::default();
    }

    #[test]
    fn test_wrap_transfer() {
        let mut context = get_context(accounts(2));
        testing_env!(context.build());
        let platform_id = accounts(4);
        let token_id = accounts(5);
        let mut contract = Contract::new_default_meta(
            accounts(2).into(),
            "YOUR MOM".to_string(),
            "YOUR MOM".to_string(),
            None,
            vec![TokenWithRatioValid { token_id: token_id.clone(), ratio: 1 }],
            0.into(),
            platform_id,
            0.into(),
        );

        let amount_transfer = 100;
        contract.balances.increase_balance(
            &accounts(1).to_string(),
            &token_id.clone().to_string(),
            amount_transfer,
        );

        testing_env!(context
            .storage_usage(env::storage_usage())
            .attached_deposit(contract.storage_balance_bounds().min.into())
            .predecessor_account_id(accounts(2))
            .build());
        // Paying for account registration, aka storage deposit
        contract.storage_deposit(None, None);

        testing_env!(context
            .storage_usage(env::storage_usage())
            .attached_deposit(contract.storage_balance_bounds().min.into())
            .predecessor_account_id(accounts(4))
            .build());
        // Paying for account registration, aka storage deposit
        contract.storage_deposit(None, None);

        testing_env!(context
            .storage_usage(env::storage_usage())
            .attached_deposit(contract.storage_balance_bounds().min.into())
            .predecessor_account_id(accounts(1))
            .build());
        // Paying for account registration, aka storage deposit
        contract.storage_deposit(None, None);
        assert_eq!(
            contract
                .balances
                .get_ft_balance(&accounts(1).to_string(), &token_id.clone().to_string()),
            amount_transfer
        );

        testing_env!(context
            .storage_usage(env::storage_usage())
            .attached_deposit(contract.storage_balance_bounds().min.into())
            .predecessor_account_id(accounts(1))
            .build());
        // Paying for account registration, aka storage deposit
        contract.storage_deposit(None, None);
        assert_eq!(
            contract
                .balances
                .get_ft_balance(&accounts(1).to_string(), &token_id.clone().to_string()),
            amount_transfer
        );

        testing_env!(context
            .storage_usage(env::storage_usage())
            .attached_deposit(1)
            .predecessor_account_id(accounts(1))
            .build());
        // Paying for account registration, aka storage deposit

        contract.wrap(None);
        assert_eq!(
            contract
                .balances
                .get_ft_balance(&accounts(1).to_string(), &token_id.clone().to_string()),
            0
        );

        // TODO: with wrap

        testing_env!(context
            .storage_usage(env::storage_usage())
            .attached_deposit(1)
            .predecessor_account_id(accounts(1))
            .build());
        assert_eq!(contract.ft_balance_of(accounts(1)).0, amount_transfer);
        contract.ft_transfer(accounts(2), amount_transfer.into(), None);

        testing_env!(context
            .storage_usage(env::storage_usage())
            .account_balance(env::account_balance())
            .is_view(true)
            .attached_deposit(0)
            .build());
        assert_eq!(contract.ft_balance_of(accounts(1)).0, (0));
        assert_eq!(contract.ft_balance_of(accounts(2)).0, amount_transfer);
    }
}
