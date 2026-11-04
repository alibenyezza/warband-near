//! WARBAND: a NEP-141 memecoin where the mechanic lives in the contract.
//!
//! - Every balance is a stack of lots. A lot gains one level per "day" held (0 to 10, the day length is fixed at
//!   launch). Outflows (transfer, sell) consume the youngest lots first (LIFO), so selling only resets what is sold.
//!   Tokens received by transfer start a new lot at level 0: splitting a bag across accounts creates no levels.
//! - Voting weight = Σ lot × level. It is pushed to a Sputnik DAO v2 as delegated weight (this contract is the DAO's
//!   `staking_id`), so the DAO's token-weighted votes count seniority, not just size.
//! - The token is its own market: buy and sell against NEAR on a virtual constant-product curve, 10% fee per trade.
//!   Part of the fee goes to the DAO treasury (the war chest), the rest to a buyback reserve.
//! - The buyback is a rule, not a decision: anyone may call `buyback`, and it only spends when the price is under
//!   the floor (a drawdown from the recent high), a chunk at a time, within a daily cap. Bought tokens are burned.
//! - Raids (see raids.rs and RAIDS.md): one raid per day. Holders join with their seasoned balance; when enough of
//!   the supply and enough accounts show up, raiders are minted a share and win loot pulls from the raid chest, and
//!   the streak grows. A missed raid burns a slice of the chest and resets the streak. Badges are soulbound NEP-171
//!   tokens that boost loot odds.
//! - No owner, no admin method, no upgrade path: every parameter is set by `new` and readable with `config`.
//!   The deploy script deletes the account's full-access key after init.

mod math;
#[cfg(test)]
mod raid_tests;
mod raids;
#[cfg(test)]
mod tests;

use math::{bps, mul_div};
use near_sdk::json_types::{U128, U64};
use near_sdk::serde_json::{self, json};
use near_sdk::store::{IterableMap, LookupMap, Vector};
pub use raids::{Badge, Joined, RaidConfig, RaidState};
use near_sdk::{
    env, ext_contract, near, require, AccountId, BorshStorageKey, Gas, NearToken, PanicOnDefault, Promise,
    PromiseOrValue,
};

pub const MAX_LEVEL: u8 = 10;
pub const MAX_LOTS: usize = 32;
const HISTORY_SIZE: u64 = 100;
const DECIMALS: u8 = 18;
const ONE_TOKEN: u128 = 10u128.pow(DECIMALS as u32);
/// Flat storage deposit per holder: account entry, up to MAX_LOTS lots, delegation record, raid entry and up to
/// four badges.
pub const STORAGE_DEPOSIT: u128 = 15_000_000_000_000_000_000_000; // 0.015 NEAR
const MIN_DAO_FLUSH: u128 = 100_000_000_000_000_000_000_000; // 0.1 NEAR

const GAS_FOR_RESOLVE_TRANSFER: Gas = Gas::from_tgas(10);
const GAS_FOR_FT_TRANSFER_CALL: Gas = Gas::from_tgas(35);
const GAS_FOR_DAO_CALL: Gas = Gas::from_tgas(10);
const GAS_FOR_CALLBACK: Gas = Gas::from_tgas(10);

#[derive(BorshStorageKey)]
#[near]
enum StorageKey {
    Accounts,
    History,
    Raids,
    Badges,
}

/// Launch parameters. Set once by `new`, never writable afterwards.
#[near(serializers = [borsh, json])]
#[derive(Clone)]
pub struct Config {
    pub name: String,
    pub symbol: String,
    pub icon: Option<String>,
    /// Length of one level step, in seconds (86400 in production, a few minutes for the demo).
    pub day_seconds: u64,
    /// Total token units on the curve at launch (18 decimals).
    pub curve_supply: U128,
    /// Virtual NEAR reserve of the curve at launch, in yoctoNEAR. Sets the starting price.
    pub virtual_near: U128,
    /// Trading fee in basis points (1000 = 10%).
    pub fee_bps: u16,
    /// Share of the fee sent to the DAO treasury, in basis points of the fee. The rest funds buybacks.
    pub dao_share_bps: u16,
    /// Buyback opens when the price is this far under the recent high (basis points).
    pub floor_drawdown_bps: u16,
    /// Each buyback call spends this share of the reserve (basis points).
    pub buyback_chunk_bps: u16,
    /// At most this share of the reserve, measured at the first buyback of the day, is spent per day.
    pub buyback_daily_cap_bps: u16,
    /// Sputnik DAO v2 that receives the war chest share and the level-weighted vote weight. None = no DAO.
    pub dao_id: Option<AccountId>,
    /// Raid rules (raids.rs).
    pub raid: RaidConfig,
}

#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub struct Lot {
    pub amount: U128,
    /// Block timestamp (ns) when the lot was opened.
    pub since: U64,
}

#[near(serializers = [borsh])]
pub struct Holder {
    pub lots: Vec<Lot>,
    pub storage: u128,
    /// Weight last pushed to the DAO (optimistic, rolled back by the callback on failure).
    pub delegated: u128,
    pub dao_registered: bool,
    /// The raid this holder joined and is not yet paid for.
    pub joined: Option<Joined>,
    /// Indexes of this holder's badges in `badges`.
    pub badges: Vec<u32>,
}

#[near(serializers = [borsh, json])]
#[derive(Clone)]
pub struct Entry {
    pub n: U64,
    pub kind: String,
    pub account: AccountId,
    pub near: U128,
    pub tokens: U128,
    pub price: U128,
    pub at: U64,
}

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct Contract {
    config: Config,
    genesis: u64,
    accounts: IterableMap<AccountId, Holder>,
    /// Curve reserves: virtual + real NEAR, and unsold tokens.
    near_reserve: u128,
    token_reserve: u128,
    circulating: u128,
    burned: u128,
    /// NEAR set aside by fees.
    buyback_reserve: u128,
    dao_pending: u128,
    dao_paid: u128,
    /// Highest price of the current and previous day, for the floor.
    high_day: u64,
    high_today: u128,
    high_prev: u128,
    /// Daily buyback budget.
    bb_day: u64,
    bb_cap: u128,
    bb_spent: u128,
    history: LookupMap<u64, Entry>,
    history_len: u64,
    /// Raid chest (NEAR from fees) and the raid ledger.
    chest: u128,
    raids: LookupMap<u64, RaidState>,
    /// Every raid before this one is settled.
    raids_settled: u64,
    streak: u32,
    best_streak: u32,
    raids_won: u32,
    raids_lost: u32,
    minted: u128,
    pull_nonce: u64,
    badges: Vector<Badge>,
}

#[ext_contract(ext_dao)]
pub trait SputnikDao {
    fn register_delegation(&mut self, account_id: AccountId);
    fn delegate(&mut self, account_id: AccountId, amount: U128) -> (U128, U128, U128);
    fn undelegate(&mut self, account_id: AccountId, amount: U128) -> (U128, U128, U128);
}

#[ext_contract(ext_ft_receiver)]
pub trait FungibleTokenReceiver {
    fn ft_on_transfer(&mut self, sender_id: AccountId, amount: U128, msg: String) -> PromiseOrValue<U128>;
}

#[ext_contract(ext_self)]
pub trait SelfCallbacks {
    fn on_dao_registered(&mut self, account_id: AccountId);
    fn on_delegation(&mut self, account_id: AccountId, previous: U128);
    fn on_dao_flush(&mut self, amount: U128);
}

#[near(serializers = [json])]
pub struct StorageBalance {
    pub total: U128,
    pub available: U128,
}

#[near(serializers = [json])]
pub struct StorageBalanceBounds {
    pub min: U128,
    pub max: Option<U128>,
}

#[near(serializers = [json])]
pub struct FungibleTokenMetadata {
    pub spec: String,
    pub name: String,
    pub symbol: String,
    pub icon: Option<String>,
    pub reference: Option<String>,
    pub reference_hash: Option<String>,
    pub decimals: u8,
}

#[near(serializers = [json])]
pub struct LotView {
    pub amount: U128,
    pub since: U64,
    pub level: u8,
    pub rank: String,
}

#[near(serializers = [json])]
pub struct AccountView {
    pub account_id: AccountId,
    pub balance: U128,
    pub weight: U128,
    pub top_level: u8,
    pub rank: String,
    pub lots: Vec<LotView>,
    pub delegated: U128,
    pub dao_registered: bool,
}

#[near(serializers = [json])]
pub struct MarketView {
    pub day_index: U64,
    pub price: U128,
    pub reference_high: U128,
    pub floor_price: U128,
    pub buyback_open: bool,
    pub near_reserve: U128,
    pub token_reserve: U128,
    pub real_near: U128,
    pub circulating: U128,
    pub burned: U128,
    pub buyback_reserve: U128,
    pub buyback_spent_today: U128,
    pub buyback_cap_today: U128,
    pub dao_pending: U128,
    pub dao_paid: U128,
    pub holders: u32,
}

pub const RANKS: [&str; 11] = [
    "Recruit", "Soldier", "Corporal", "Sergeant", "Lieutenant", "Captain", "Commander", "Colonel", "General",
    "Marshal", "Warlord",
];

#[near]
impl Contract {
    #[init]
    pub fn new(config: Config) -> Self {
        require!(config.day_seconds >= 60, "day_seconds must be at least 60");
        require!(config.curve_supply.0 > 0 && config.virtual_near.0 > 0, "empty curve");
        require!(config.fee_bps <= 2_000, "fee above 20%");
        require!(config.dao_share_bps <= 10_000, "dao_share_bps above 100%");
        require!(config.dao_id.is_some() || config.dao_share_bps == 0, "dao share without a dao");
        require!(config.floor_drawdown_bps > 0 && config.floor_drawdown_bps < 10_000, "bad floor");
        require!(config.buyback_chunk_bps > 0 && config.buyback_chunk_bps <= 10_000, "bad chunk");
        require!(config.buyback_daily_cap_bps > 0 && config.buyback_daily_cap_bps <= 10_000, "bad daily cap");
        config.raid.validate();
        let near_reserve = config.virtual_near.0;
        let token_reserve = config.curve_supply.0;
        let price = mul_div(near_reserve, ONE_TOKEN, token_reserve);
        Self {
            config,
            genesis: env::block_timestamp(),
            accounts: IterableMap::new(StorageKey::Accounts),
            near_reserve,
            token_reserve,
            circulating: 0,
            burned: 0,
            buyback_reserve: 0,
            dao_pending: 0,
            dao_paid: 0,
            high_day: 0,
            high_today: price,
            high_prev: 0,
            bb_day: u64::MAX,
            bb_cap: 0,
            bb_spent: 0,
            history: LookupMap::new(StorageKey::History),
            history_len: 0,
            chest: 0,
            raids: LookupMap::new(StorageKey::Raids),
            // day 0 is launch day: nobody can hold seasoned tokens yet, so the first raid is day 1
            raids_settled: 1,
            streak: 0,
            best_streak: 0,
            raids_won: 0,
            raids_lost: 0,
            minted: 0,
            pull_nonce: 0,
            badges: Vector::new(StorageKey::Badges),
        }
    }

    // ---------------------------------------------------------------- market

    /// Buy tokens with the attached NEAR. Registers the caller first if needed (the storage deposit is taken
    /// from the attachment). 10% fee on the NEAR paid in.
    #[payable]
    pub fn buy(&mut self, min_tokens_out: Option<U128>) -> U128 {
        let buyer = env::predecessor_account_id();
        let mut deposit = env::attached_deposit().as_yoctonear();
        if !self.accounts.contains_key(&buyer) {
            require!(deposit > STORAGE_DEPOSIT, "attach more than the 0.015 NEAR storage deposit");
            deposit -= STORAGE_DEPOSIT;
            self.register(&buyer, STORAGE_DEPOSIT);
        }
        require!(deposit > 0, "attach NEAR to buy");
        self.roll_day();
        let fee = bps(deposit, self.config.fee_bps);
        self.take_fee(fee);
        let net = deposit - fee;
        let out = mul_div(self.token_reserve, net, self.near_reserve + net);
        require!(out > 0, "amount too small");
        if let Some(min) = min_tokens_out {
            require!(out >= min.0, "slippage: fewer tokens than min_tokens_out");
        }
        self.near_reserve += net;
        self.token_reserve -= out;
        self.circulating += out;
        self.credit(&buyer, out);
        self.track_high();
        log_ft("ft_mint", json!([{ "owner_id": buyer, "amount": U128(out), "memo": "buy" }]));
        self.record("buy", &buyer, deposit, out);
        U128(out)
    }

    /// Sell tokens back to the curve. Requires one yoctoNEAR, so only a full-access key can sell.
    #[payable]
    pub fn sell(&mut self, amount: U128, min_near_out: Option<U128>) -> U128 {
        assert_one_yocto();
        let seller = env::predecessor_account_id();
        require!(amount.0 > 0, "amount must be positive");
        self.roll_day();
        self.debit(&seller, amount.0);
        let gross = mul_div(self.near_reserve, amount.0, self.token_reserve + amount.0);
        require!(gross <= self.near_reserve - self.config.virtual_near.0, "curve has no NEAR for this sale");
        let fee = bps(gross, self.config.fee_bps);
        let out = gross - fee;
        if let Some(min) = min_near_out {
            require!(out >= min.0, "slippage: less NEAR than min_near_out");
        }
        self.near_reserve -= gross;
        self.token_reserve += amount.0;
        self.circulating -= amount.0;
        self.take_fee(fee);
        log_ft("ft_burn", json!([{ "owner_id": seller, "amount": amount, "memo": "sell" }]));
        self.record("sell", &seller, out, amount.0);
        self.sync_down(&seller);
        Promise::new(seller).transfer(NearToken::from_yoctonear(out + 1)).detach();
        U128(out)
    }

    /// Spend one chunk of the buyback reserve on the curve and burn what it buys. Anyone can call it; it only
    /// does something when the price is under the floor and today's cap is not used up.
    pub fn buyback(&mut self) -> U128 {
        self.roll_day();
        let price = self.price();
        require!(price < self.floor_price(), "price is above the floor: no buyback");
        let today = self.day_index();
        if self.bb_day != today {
            self.bb_day = today;
            self.bb_spent = 0;
            self.bb_cap = bps(self.buyback_reserve, self.config.buyback_daily_cap_bps);
        }
        let chunk = bps(self.buyback_reserve, self.config.buyback_chunk_bps);
        let spend = chunk.min(self.bb_cap - self.bb_spent).min(self.buyback_reserve);
        require!(spend > 0, "nothing to spend today");
        let out = mul_div(self.token_reserve, spend, self.near_reserve + spend);
        require!(out > 0, "buyback too small");
        self.buyback_reserve -= spend;
        self.bb_spent += spend;
        self.near_reserve += spend;
        self.token_reserve -= out;
        self.burned += out;
        let caller = env::predecessor_account_id();
        log_ft("ft_burn", json!([{ "owner_id": env::current_account_id(), "amount": U128(out), "memo": "buyback" }]));
        self.record("buyback", &caller, spend, out);
        U128(out)
    }

    /// Send the war chest share collected so far to the DAO. Anyone can call it.
    pub fn flush_to_dao(&mut self) -> Promise {
        let dao = self.config.dao_id.clone().expect("no dao configured");
        let amount = self.dao_pending;
        require!(amount >= MIN_DAO_FLUSH, "less than 0.1 NEAR pending");
        self.dao_pending = 0;
        self.dao_paid += amount;
        self.record("flush", &dao, amount, 0);
        Promise::new(dao).transfer(NearToken::from_yoctonear(amount)).then(
            ext_self::ext(env::current_account_id())
                .with_static_gas(GAS_FOR_CALLBACK)
                .on_dao_flush(U128(amount)),
        )
    }

    #[private]
    pub fn on_dao_flush(&mut self, amount: U128) {
        if !promise_ok() {
            self.dao_pending += amount.0;
            self.dao_paid -= amount.0;
        }
    }

    // ---------------------------------------------------------------- DAO weight

    /// Register the caller as a delegator in the DAO. Attach exactly what Sputnik requires (16 bytes of storage);
    /// any surplus is refunded.
    #[payable]
    pub fn dao_register(&mut self) -> Promise {
        let account = env::predecessor_account_id();
        let dao = self.config.dao_id.clone().expect("no dao configured");
        let holder = self.accounts.get(&account).expect("account not registered");
        require!(!holder.dao_registered, "already registered in the dao");
        let cost = env::storage_byte_cost().saturating_mul(16);
        let attached = env::attached_deposit();
        require!(attached >= cost, "attach 16 bytes of storage cost");
        if attached > cost {
            Promise::new(account.clone()).transfer(attached.saturating_sub(cost)).detach();
        }
        ext_dao::ext(dao)
            .with_attached_deposit(cost)
            .with_static_gas(GAS_FOR_DAO_CALL)
            .register_delegation(account.clone())
            .then(ext_self::ext(env::current_account_id()).with_static_gas(GAS_FOR_CALLBACK).on_dao_registered(account))
    }

    #[private]
    pub fn on_dao_registered(&mut self, account_id: AccountId) {
        if promise_ok() {
            if let Some(h) = self.accounts.get_mut(&account_id) {
                h.dao_registered = true;
            }
        }
    }

    /// Push an account's current level-weighted weight to the DAO. Anyone can call it for anyone: the weight only
    /// depends on the account's lots and the clock.
    pub fn sync_weight(&mut self, account_id: AccountId) -> Option<Promise> {
        let holder = self.accounts.get(&account_id).expect("account not registered");
        require!(holder.dao_registered, "account not registered in the dao");
        self.push_weight(&account_id)
    }

    #[private]
    pub fn on_delegation(&mut self, account_id: AccountId, previous: U128) {
        if !promise_ok() {
            if let Some(h) = self.accounts.get_mut(&account_id) {
                h.delegated = previous.0;
            }
        }
    }

    // ---------------------------------------------------------------- NEP-141

    #[payable]
    pub fn ft_transfer(&mut self, receiver_id: AccountId, amount: U128, memo: Option<String>) {
        assert_one_yocto();
        let sender = env::predecessor_account_id();
        self.internal_transfer(&sender, &receiver_id, amount.0, memo);
    }

    #[payable]
    pub fn ft_transfer_call(
        &mut self,
        receiver_id: AccountId,
        amount: U128,
        memo: Option<String>,
        msg: String,
    ) -> PromiseOrValue<U128> {
        assert_one_yocto();
        require!(env::prepaid_gas() > GAS_FOR_FT_TRANSFER_CALL, "more gas is required");
        let sender = env::predecessor_account_id();
        self.internal_transfer(&sender, &receiver_id, amount.0, memo);
        ext_ft_receiver::ext(receiver_id.clone())
            .with_static_gas(env::prepaid_gas().saturating_sub(GAS_FOR_FT_TRANSFER_CALL))
            .ft_on_transfer(sender.clone(), amount, msg)
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(GAS_FOR_RESOLVE_TRANSFER)
                    .ft_resolve_transfer(sender, receiver_id, amount),
            )
            .into()
    }

    /// Refunded tokens come back to the sender as a new lot (level 0): the receiver held them for a moment.
    #[private]
    pub fn ft_resolve_transfer(&mut self, sender_id: AccountId, receiver_id: AccountId, amount: U128) -> U128 {
        let unused = match env::promise_result_checked(0, 64) {
            Ok(bytes) => serde_json::from_slice::<U128>(&bytes).map(|u| u.0.min(amount.0)).unwrap_or(amount.0),
            Err(_) => amount.0,
        };
        if unused == 0 {
            return amount;
        }
        let receiver_balance = self.balance(&receiver_id);
        let refund = unused.min(receiver_balance);
        if refund == 0 {
            return amount;
        }
        self.debit(&receiver_id, refund);
        if self.accounts.contains_key(&sender_id) {
            self.credit(&sender_id, refund);
            log_ft(
                "ft_transfer",
                json!([{ "old_owner_id": receiver_id, "new_owner_id": sender_id, "amount": U128(refund), "memo": "refund" }]),
            );
        } else {
            self.circulating -= refund;
            self.burned += refund;
            log_ft("ft_burn", json!([{ "owner_id": receiver_id, "amount": U128(refund), "memo": "refund to unregistered sender" }]));
        }
        self.sync_down(&receiver_id);
        U128(amount.0 - refund)
    }

    pub fn ft_total_supply(&self) -> U128 {
        U128(self.circulating)
    }

    pub fn ft_balance_of(&self, account_id: AccountId) -> U128 {
        U128(self.balance(&account_id))
    }

    pub fn ft_metadata(&self) -> FungibleTokenMetadata {
        FungibleTokenMetadata {
            spec: "ft-1.0.0".into(),
            name: self.config.name.clone(),
            symbol: self.config.symbol.clone(),
            icon: self.config.icon.clone(),
            reference: None,
            reference_hash: None,
            decimals: DECIMALS,
        }
    }

    // ---------------------------------------------------------------- NEP-145

    #[payable]
    pub fn storage_deposit(&mut self, account_id: Option<AccountId>, registration_only: Option<bool>) -> StorageBalance {
        let _ = registration_only; // min == max: any surplus is always refunded
        let account = account_id.unwrap_or_else(env::predecessor_account_id);
        let attached = env::attached_deposit().as_yoctonear();
        if self.accounts.contains_key(&account) {
            if attached > 0 {
                Promise::new(env::predecessor_account_id()).transfer(NearToken::from_yoctonear(attached)).detach();
            }
        } else {
            require!(attached >= STORAGE_DEPOSIT, "attach at least 0.015 NEAR");
            self.register(&account, STORAGE_DEPOSIT);
            if attached > STORAGE_DEPOSIT {
                Promise::new(env::predecessor_account_id())
                    .transfer(NearToken::from_yoctonear(attached - STORAGE_DEPOSIT))
                    .detach();
            }
        }
        self.storage_balance_of(account).unwrap()
    }

    /// Nothing is ever withdrawable: the deposit is flat (min == max).
    #[payable]
    pub fn storage_withdraw(&mut self, amount: Option<U128>) -> StorageBalance {
        assert_one_yocto();
        let account = env::predecessor_account_id();
        require!(amount.map(|a| a.0).unwrap_or(0) == 0, "nothing to withdraw");
        self.storage_balance_of(account).expect("account not registered")
    }

    /// Unregister and get the storage deposit back. With a balance, `force` burns it.
    #[payable]
    pub fn storage_unregister(&mut self, force: Option<bool>) -> bool {
        assert_one_yocto();
        let account = env::predecessor_account_id();
        let Some(holder) = self.accounts.get(&account) else { return false };
        require!(holder.delegated == 0, "undelegate first: sell or move the tokens, then sync_weight");
        let balance = total(&holder.lots);
        require!(balance == 0 || force.unwrap_or(false), "balance is not empty, use force to burn it");
        let storage = holder.storage;
        self.accounts.remove(&account);
        if balance > 0 {
            self.circulating -= balance;
            self.burned += balance;
            log_ft("ft_burn", json!([{ "owner_id": account, "amount": U128(balance), "memo": "unregister" }]));
        }
        Promise::new(account).transfer(NearToken::from_yoctonear(storage + 1)).detach();
        true
    }

    pub fn storage_balance_bounds(&self) -> StorageBalanceBounds {
        StorageBalanceBounds { min: U128(STORAGE_DEPOSIT), max: Some(U128(STORAGE_DEPOSIT)) }
    }

    pub fn storage_balance_of(&self, account_id: AccountId) -> Option<StorageBalance> {
        self.accounts.get(&account_id).map(|h| StorageBalance { total: U128(h.storage), available: U128(0) })
    }

    // ---------------------------------------------------------------- views

    pub fn config(&self) -> Config {
        self.config.clone()
    }

    pub fn weight_of(&self, account_id: AccountId) -> U128 {
        U128(self.accounts.get(&account_id).map(|h| self.weight(&h.lots)).unwrap_or(0))
    }

    pub fn level_of(&self, since: U64) -> u8 {
        self.level(since.0)
    }

    pub fn account(&self, account_id: AccountId) -> Option<AccountView> {
        self.accounts.get(&account_id).map(|h| self.view_holder(&account_id, h))
    }

    /// Holders in storage order; sort client side.
    pub fn holders(&self, from_index: Option<u32>, limit: Option<u32>) -> Vec<AccountView> {
        self.accounts
            .iter()
            .skip(from_index.unwrap_or(0) as usize)
            .take(limit.unwrap_or(50).min(100) as usize)
            .map(|(a, h)| self.view_holder(a, h))
            .collect()
    }

    pub fn market(&self) -> MarketView {
        let today = self.day_index();
        let (spent, cap) = if self.bb_day == today {
            (self.bb_spent, self.bb_cap)
        } else {
            (0, bps(self.buyback_reserve, self.config.buyback_daily_cap_bps))
        };
        MarketView {
            day_index: U64(today),
            price: U128(self.price()),
            reference_high: U128(self.reference_high()),
            floor_price: U128(self.floor_price()),
            buyback_open: self.price() < self.floor_price() && self.buyback_reserve > 0 && spent < cap,
            near_reserve: U128(self.near_reserve),
            token_reserve: U128(self.token_reserve),
            real_near: U128(self.near_reserve - self.config.virtual_near.0),
            circulating: U128(self.circulating),
            burned: U128(self.burned),
            buyback_reserve: U128(self.buyback_reserve),
            buyback_spent_today: U128(spent),
            buyback_cap_today: U128(cap),
            dao_pending: U128(self.dao_pending),
            dao_paid: U128(self.dao_paid),
            holders: self.accounts.len(),
        }
    }

    pub fn quote_buy(&self, near_in: U128) -> U128 {
        let net = near_in.0 - bps(near_in.0, self.config.fee_bps);
        U128(mul_div(self.token_reserve, net, self.near_reserve + net))
    }

    pub fn quote_sell(&self, amount: U128) -> U128 {
        let gross = mul_div(self.near_reserve, amount.0, self.token_reserve + amount.0);
        U128(gross - bps(gross, self.config.fee_bps))
    }

    /// The last actions, newest first (ring buffer of 100).
    pub fn history(&self, limit: Option<u32>) -> Vec<Entry> {
        let n = self.history_len;
        let take = (limit.unwrap_or(50) as u64).min(HISTORY_SIZE).min(n);
        (0..take).filter_map(|i| self.history.get(&((n - 1 - i) % HISTORY_SIZE)).cloned()).collect()
    }
}

// -------------------------------------------------------------------- internals

impl Contract {
    fn register(&mut self, account: &AccountId, storage: u128) {
        self.accounts
            .insert(account.clone(), Holder { lots: vec![], storage, delegated: 0, dao_registered: false, joined: None, badges: vec![] });
    }

    fn balance(&self, account: &AccountId) -> u128 {
        self.accounts.get(account).map(|h| total(&h.lots)).unwrap_or(0)
    }

    fn day_index(&self) -> u64 {
        (env::block_timestamp() - self.genesis) / self.day_ns()
    }

    fn day_ns(&self) -> u64 {
        self.config.day_seconds * 1_000_000_000
    }

    fn level(&self, since: u64) -> u8 {
        let held = env::block_timestamp().saturating_sub(since) / self.day_ns();
        held.min(MAX_LEVEL as u64) as u8
    }

    fn weight(&self, lots: &[Lot]) -> u128 {
        lots.iter().map(|l| l.amount.0 * self.level(l.since.0) as u128).sum()
    }

    fn view_holder(&self, account: &AccountId, h: &Holder) -> AccountView {
        let lots: Vec<LotView> = h
            .lots
            .iter()
            .map(|l| {
                let level = self.level(l.since.0);
                LotView { amount: l.amount, since: l.since, level, rank: RANKS[level as usize].into() }
            })
            .collect();
        let top_level = lots.iter().map(|l| l.level).max().unwrap_or(0);
        AccountView {
            account_id: account.clone(),
            balance: U128(total(&h.lots)),
            weight: U128(self.weight(&h.lots)),
            top_level,
            rank: RANKS[top_level as usize].into(),
            lots,
            delegated: U128(h.delegated),
            dao_registered: h.dao_registered,
        }
    }

    /// New tokens open a lot at level 0. Same-day lots merge (keeping the newer timestamp, never older); past
    /// MAX_LOTS the two youngest lots merge the same way, so storage stays bounded and nobody gains age.
    fn credit(&mut self, account: &AccountId, amount: u128) {
        let now = env::block_timestamp();
        let day_ns = self.day_ns();
        let genesis = self.genesis;
        let holder = self.accounts.get_mut(account).expect("account not registered");
        let same_day = holder.lots.last().map(|l| (l.since.0 - genesis) / day_ns == (now - genesis) / day_ns);
        if same_day == Some(true) {
            let last = holder.lots.last_mut().unwrap();
            last.amount = U128(last.amount.0 + amount);
            last.since = U64(now);
        } else {
            holder.lots.push(Lot { amount: U128(amount), since: U64(now) });
        }
        if holder.lots.len() > MAX_LOTS {
            let young = holder.lots.pop().unwrap();
            let prev = holder.lots.last_mut().unwrap();
            prev.amount = U128(prev.amount.0 + young.amount.0);
            prev.since = U64(prev.since.0.max(young.since.0));
        }
    }

    /// Outflows consume the youngest lots first (LIFO).
    fn debit(&mut self, account: &AccountId, amount: u128) {
        let holder = self.accounts.get_mut(account).expect("account not registered");
        require!(total(&holder.lots) >= amount, "not enough tokens");
        let mut left = amount;
        while left > 0 {
            let last = holder.lots.last_mut().unwrap();
            if last.amount.0 <= left {
                left -= last.amount.0;
                holder.lots.pop();
            } else {
                last.amount = U128(last.amount.0 - left);
                left = 0;
            }
        }
    }

    fn internal_transfer(&mut self, sender: &AccountId, receiver: &AccountId, amount: u128, memo: Option<String>) {
        require!(sender != receiver, "sender and receiver must differ");
        require!(amount > 0, "amount must be positive");
        require!(self.accounts.contains_key(receiver), "receiver not registered: call storage_deposit first");
        self.debit(sender, amount);
        self.credit(receiver, amount);
        log_ft(
            "ft_transfer",
            json!([{ "old_owner_id": sender, "new_owner_id": receiver, "amount": U128(amount), "memo": memo }]),
        );
        self.record("transfer", sender, 0, amount);
        self.sync_down(sender);
    }

    /// After an outflow, pull back any delegated weight the account no longer has.
    fn sync_down(&mut self, account: &AccountId) {
        let Some(h) = self.accounts.get(account) else { return };
        if h.dao_registered && self.weight(&h.lots) < h.delegated {
            if let Some(p) = self.push_weight(account) {
                p.detach();
            }
        }
    }

    fn push_weight(&mut self, account: &AccountId) -> Option<Promise> {
        let dao = self.config.dao_id.clone()?;
        let weight = self.weight(&self.accounts.get(account)?.lots);
        let holder = self.accounts.get_mut(account)?;
        let previous = holder.delegated;
        if weight == previous {
            return None;
        }
        holder.delegated = weight;
        let call = ext_dao::ext(dao).with_static_gas(GAS_FOR_DAO_CALL);
        let call = if weight > previous {
            call.delegate(account.clone(), U128(weight - previous))
        } else {
            call.undelegate(account.clone(), U128(previous - weight))
        };
        Some(call.then(
            ext_self::ext(env::current_account_id())
                .with_static_gas(GAS_FOR_CALLBACK)
                .on_delegation(account.clone(), U128(previous)),
        ))
    }

    fn take_fee(&mut self, fee: u128) {
        let to_chest = bps(fee, self.config.raid.chest_share_bps);
        self.chest += to_chest;
        let fee = fee - to_chest;
        let to_dao = bps(fee, self.config.dao_share_bps);
        self.dao_pending += to_dao;
        self.buyback_reserve += fee - to_dao;
    }

    fn price(&self) -> u128 {
        mul_div(self.near_reserve, ONE_TOKEN, self.token_reserve)
    }

    fn roll_day(&mut self) {
        self.roll_raids();
        let today = self.day_index();
        if today != self.high_day {
            self.high_prev = if today == self.high_day + 1 { self.high_today } else { 0 };
            self.high_day = today;
            self.high_today = self.price();
        }
    }

    fn track_high(&mut self) {
        self.high_today = self.high_today.max(self.price());
    }

    fn reference_high(&self) -> u128 {
        let today = self.day_index();
        if today == self.high_day {
            self.high_today.max(self.high_prev)
        } else if today == self.high_day + 1 {
            self.high_today.max(self.price())
        } else {
            self.price()
        }
    }

    fn floor_price(&self) -> u128 {
        mul_div(self.reference_high(), (10_000 - self.config.floor_drawdown_bps) as u128, 10_000)
    }

    fn record(&mut self, kind: &str, account: &AccountId, near: u128, tokens: u128) {
        self.store_entry(kind, account, near, tokens);
        env::log_str(&format!(
            "EVENT_JSON:{}",
            json!({ "standard": "warband", "version": "1.0.0", "event": kind,
                    "data": [{ "account_id": account, "near": U128(near), "tokens": U128(tokens), "price": U128(self.price()) }] })
        ));
    }

    /// Adds a line to the public log without an event (for actions that already emit their own).
    pub(crate) fn store_entry(&mut self, kind: &str, account: &AccountId, near: u128, tokens: u128) {
        let n = self.history_len;
        let entry = Entry {
            n: U64(n),
            kind: kind.into(),
            account: account.clone(),
            near: U128(near),
            tokens: U128(tokens),
            price: U128(self.price()),
            at: U64(env::block_timestamp()),
        };
        self.history.insert(n % HISTORY_SIZE, entry);
        self.history_len += 1;
    }
}

fn total(lots: &[Lot]) -> u128 {
    lots.iter().map(|l| l.amount.0).sum()
}

fn assert_one_yocto() {
    require!(env::attached_deposit() == NearToken::from_yoctonear(1), "requires exactly 1 yoctoNEAR (full-access key)");
}

fn promise_ok() -> bool {
    env::promise_results_count() == 1 && matches!(env::promise_result_checked(0, 1024), Ok(_))
}

fn log_ft(event: &str, data: serde_json::Value) {
    env::log_str(&format!(
        "EVENT_JSON:{}",
        json!({ "standard": "nep141", "version": "1.0.0", "event": event, "data": data })
    ));
}

