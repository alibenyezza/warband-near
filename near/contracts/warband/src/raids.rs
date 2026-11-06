//! Raids: the daily game of the band (rules in near/RAIDS.md, parameters chosen with near/sim/raids.py).
//!
//! - One raid per day (`day_seconds`). A holder joins with its seasoned balance: tokens held at least one day.
//!   Fresh tokens (bought, minted or received today) do not count, so a bag cannot be split across accounts to join
//!   twice: a transfer opens a level-0 lot at the receiver.
//! - The raid's target is fixed when the raid opens: `target_bps` of the circulating supply, plus `min_joiners`
//!   accounts. The raid is settled by the first action after it ends.
//! - Won: the streak grows; each raider is minted `mint_bps` of its joined balance (x1.1 at streak 1 up to x2 at
//!   streak 10) as a fresh lot, and the raid's `raid_pulls` loot pulls are shared among raiders in proportion to
//!   their joined balance (whole pulls, the fraction decided by a draw). Lost: `fail_burn_bps` of the chest buys
//!   tokens on the curve and burns them, and the streak resets.
//! - A pull pays a share of the chest by tier. Higher level and badges raise epic and legendary odds (up to x2).
//!   Extra pulls cost tokens worth `ticket_cost_bps` of the chest, priced at min(spot, yesterday's high), burned.
//! - Badges are soulbound NEP-171 tokens: First Blood, Seven Days, Legendary Pull, Warlord. Each adds two levels to
//!   the loot odds, and they stay after the tokens are sold.
//!
//! Randomness comes from `env::random_seed()`, mixed with the account and a counter. A caller cannot see it before
//! its transaction runs, and cannot undo a pull it does not like (the pull commits in the same receipt). A block
//! producer could bias it: acceptable for testnet loot, written in RAIDS.md.

use crate::math::{bps, mul_div};
use crate::*;

/// Base odds per 10_000 and share of the chest paid, in basis points.
pub const TIERS: [(&str, u32, u16); 4] = [("common", 8_500, 5), ("rare", 1_200, 50), ("epic", 270, 300), ("legendary", 30, 1_500)];
/// Level used for the odds is capped here (x2 on epic and legendary).
const MAX_ODDS_LEVEL: u32 = 10;
// each burning lost day writes two logs (~330 bytes); a receipt holds 100 logs and 16 KiB, so a long empty stretch
// is settled 20 days per call (any action or settle_raids moves it on)
const MAX_SETTLE_PER_CALL: u64 = 20;
const MAX_PULLS_PER_CLAIM: u32 = 64;
const PULL_SCALE: u128 = 1_000_000;

pub const BADGES: [(&str, &str); 4] = [
    ("First Blood", "Won a first raid with the band."),
    ("Seven Days", "Won a raid on a streak of seven or more."),
    ("Legendary Pull", "Pulled a legendary from the raid chest."),
    ("Warlord", "Held a lot to level 10."),
];
pub const FIRST_BLOOD: u8 = 0;
pub const SEVEN_DAYS: u8 = 1;
pub const LEGENDARY: u8 = 2;
pub const WARLORD: u8 = 3;

#[near(serializers = [borsh, json])]
#[derive(Clone)]
pub struct RaidConfig {
    /// Share of every trading fee that goes to the raid chest, taken before the DAO and buyback split.
    pub chest_share_bps: u16,
    /// Least seasoned balance to join a raid (token units).
    pub min_join_balance: U128,
    pub min_joiners: u32,
    /// The joiners' seasoned balance must reach this share of the circulating supply at the raid's opening.
    pub target_bps: u16,
    /// Minted to each raider of a won raid, in basis points of its joined balance, before the streak bonus.
    pub mint_bps: u16,
    /// Share of the chest burned when a raid is lost.
    pub fail_burn_bps: u16,
    /// Loot pulls a won raid hands out, shared by joined balance.
    pub raid_pulls: u32,
    /// Price of an extra pull, in basis points of the chest (paid in tokens, burned).
    pub ticket_cost_bps: u16,
}

impl RaidConfig {
    pub fn validate(&self) {
        require!(self.chest_share_bps <= 10_000, "chest_share_bps above 100%");
        require!(self.min_join_balance.0 > 0, "min_join_balance must be positive");
        require!(self.min_joiners >= 1, "min_joiners must be at least 1");
        require!(self.target_bps > 0 && self.target_bps <= 10_000, "bad target_bps");
        require!(self.mint_bps <= 1_000, "mint_bps above 10%");
        require!(self.fail_burn_bps <= 10_000, "fail_burn_bps above 100%");
        require!(self.raid_pulls <= MAX_PULLS_PER_CLAIM, "raid_pulls above 64");
        require!(self.ticket_cost_bps > 0 && self.ticket_cost_bps <= 10_000, "bad ticket_cost_bps");
        // the best odds must stay under the ticket price, or extra pulls would drain the chest
        let ev = expected_pull_bps(MAX_ODDS_LEVEL);
        require!(ev < self.ticket_cost_bps as u128 * 100, "a pull is worth more than its ticket");
    }
}

/// Expected chest share of one pull at a given odds level, in 1/100 bps.
fn expected_pull_bps(level: u32) -> u128 {
    odds(level).iter().zip(TIERS.iter()).map(|(o, t)| *o as u128 * t.2 as u128 / 100).sum()
}

fn odds(level: u32) -> [u32; 4] {
    let l = level.min(MAX_ODDS_LEVEL);
    let mut o = [TIERS[0].1, TIERS[1].1, TIERS[2].1 * (10 + l) / 10, TIERS[3].1 * (10 + l) / 10];
    o[0] = 10_000 - o[1] - o[2] - o[3];
    o
}

#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub struct Joined {
    pub raid: U64,
    pub amount: U128,
}

#[near(serializers = [borsh, json])]
#[derive(Clone, Default)]
pub struct RaidState {
    /// Seasoned balance the joiners must reach.
    pub target: U128,
    pub joined: U128,
    pub joiners: u32,
    /// None while the raid runs.
    pub won: Option<bool>,
    /// Streak after this raid (its bonus for the mint).
    pub streak: u32,
    /// Tokens burned when it was lost.
    pub burned: U128,
}

#[near(serializers = [borsh, json])]
#[derive(Clone)]
pub struct Badge {
    pub owner_id: AccountId,
    pub kind: u8,
    pub issued_at: U64,
}

#[near(serializers = [json])]
pub struct RaidView {
    pub raid: U64,
    pub ends_at: U64,
    pub target: U128,
    pub joined: U128,
    pub joiners: u32,
    pub min_joiners: u32,
    pub streak: u32,
    pub best_streak: u32,
    pub won: u32,
    pub lost: u32,
    pub chest: U128,
    pub ticket_tokens: U128,
    pub minted: U128,
    pub last: Option<RaidState>,
}

#[near(serializers = [json])]
pub struct PullView {
    pub tier: String,
    pub near: U128,
}

#[near(serializers = [json])]
pub struct ClaimView {
    pub raid: U64,
    pub won: bool,
    pub minted: U128,
    pub pulls: Vec<PullView>,
    pub badges: Vec<String>,
}

#[near(serializers = [json])]
pub struct NftContractMetadata {
    pub spec: String,
    pub name: String,
    pub symbol: String,
    pub icon: Option<String>,
    pub base_uri: Option<String>,
    pub reference: Option<String>,
    pub reference_hash: Option<String>,
}

#[near(serializers = [json])]
pub struct TokenMetadata {
    pub title: Option<String>,
    pub description: Option<String>,
    pub media: Option<String>,
    pub issued_at: Option<String>,
}

#[near(serializers = [json])]
pub struct NftToken {
    pub token_id: String,
    pub owner_id: AccountId,
    pub metadata: TokenMetadata,
}

#[near]
impl Contract {
    // ---------------------------------------------------------------- raids

    /// Join today's raid with the caller's seasoned balance. Pays out the previous raid first if it is settled.
    /// No deposit: a function-call key of the account can check in every day.
    pub fn join_raid(&mut self) -> U128 {
        let account = env::predecessor_account_id();
        self.roll_day();
        let today = self.day_index();
        if let Some(j) = self.accounts.get(&account).and_then(|h| h.joined.clone()) {
            require!(j.raid.0 != today, "already in today's raid");
            self.claim_for(&account);
        }
        let holder = self.accounts.get(&account).expect("account not registered");
        let seasoned = self.seasoned(&holder.lots);
        require!(
            seasoned >= self.config.raid.min_join_balance.0,
            "not enough seasoned tokens: hold them one day before joining"
        );
        let mut raid = self.open_raid(today);
        raid.joined = U128(raid.joined.0 + seasoned);
        raid.joiners += 1;
        self.raids.insert(today, raid);
        self.accounts.get_mut(&account).unwrap().joined = Some(Joined { raid: U64(today), amount: U128(seasoned) });
        log_event("raid_join", json!({ "account_id": account, "raid": U64(today), "amount": U128(seasoned) }));
        self.store_entry("raid_join", &account, 0, seasoned);
        U128(seasoned)
    }

    /// Collect the mint and the loot of the raid the caller joined, once it is settled.
    pub fn claim_raid(&mut self) -> ClaimView {
        let account = env::predecessor_account_id();
        self.roll_day();
        self.claim_for(&account)
    }

    /// Burn tokens for one extra pull. One yoctoNEAR: only a full-access key spends tokens.
    #[payable]
    pub fn burn_for_pull(&mut self) -> PullView {
        assert_one_yocto();
        let account = env::predecessor_account_id();
        self.roll_day();
        let cost = self.ticket_tokens();
        require!(cost > 0, "the chest is empty");
        self.debit(&account, cost);
        self.circulating -= cost;
        self.burned += cost;
        log_ft("ft_burn", json!([{ "owner_id": account, "amount": U128(cost), "memo": "pull" }]));
        let mut badges = vec![];
        let pull = self.pull(&account, &mut badges);
        if pull.near.0 > 0 {
            Promise::new(account.clone()).transfer(NearToken::from_yoctonear(pull.near.0)).detach();
        }
        self.record(&format!("pull_{}", pull.tier), &account, pull.near.0, cost);
        self.sync_down(&account);
        pull
    }

    /// Mint the Warlord badge to a holder with a lot at level 10.
    pub fn claim_warlord(&mut self) -> String {
        let account = env::predecessor_account_id();
        let holder = self.accounts.get(&account).expect("account not registered");
        let top = holder.lots.iter().map(|l| self.level(l.since.0)).max().unwrap_or(0);
        require!(top >= MAX_LEVEL, "no lot at level 10 yet");
        require!(self.award(&account, WARLORD), "already a Warlord");
        BADGES[WARLORD as usize].0.into()
    }

    /// Settle every raid that has ended. Anyone can call it; every other action does it too.
    pub fn settle_raids(&mut self) -> U64 {
        self.roll_day();
        U64(self.raids_settled)
    }

    // ---------------------------------------------------------------- views

    pub fn raid_status(&self) -> RaidView {
        let today = self.day_index();
        let current = self.raids.get(&today).cloned().unwrap_or(RaidState {
            target: U128(bps(self.circulating, self.config.raid.target_bps)),
            ..Default::default()
        });
        let last = if today > 0 { self.raids.get(&(today - 1)).cloned() } else { None };
        RaidView {
            raid: U64(today),
            ends_at: U64(self.genesis + (today + 1) * self.day_ns()),
            target: current.target,
            joined: current.joined,
            joiners: current.joiners,
            min_joiners: self.config.raid.min_joiners,
            streak: self.streak,
            best_streak: self.best_streak,
            won: self.raids_won,
            lost: self.raids_lost,
            chest: U128(self.chest),
            ticket_tokens: U128(self.ticket_tokens()),
            minted: U128(self.minted),
            last,
        }
    }

    pub fn raid(&self, raid: U64) -> Option<RaidState> {
        self.raids.get(&raid.0).cloned()
    }

    pub fn raid_of(&self, account_id: AccountId) -> Option<Joined> {
        self.accounts.get(&account_id).and_then(|h| h.joined.clone())
    }

    /// This account's odds per 10_000 for common, rare, epic and legendary (level and badges included).
    pub fn pull_odds(&self, account_id: AccountId) -> Vec<u32> {
        odds(self.odds_level(&account_id)).to_vec()
    }

    // ---------------------------------------------------------------- NEP-171 (soulbound badges)

    pub fn nft_metadata(&self) -> NftContractMetadata {
        NftContractMetadata {
            spec: "nft-1.0.0".into(),
            name: format!("{} badges", self.config.name),
            symbol: format!("{}B", self.config.symbol),
            icon: None,
            base_uri: None,
            reference: None,
            reference_hash: None,
        }
    }

    pub fn nft_token(&self, token_id: String) -> Option<NftToken> {
        let i: u32 = token_id.parse().ok()?;
        self.badges.get(i).map(|b| self.view_badge(i, b))
    }

    pub fn nft_total_supply(&self) -> U128 {
        U128(self.badges.len() as u128)
    }

    pub fn nft_tokens(&self, from_index: Option<U128>, limit: Option<u32>) -> Vec<NftToken> {
        let from = from_index.map(|i| i.0 as u32).unwrap_or(0);
        let to = (from + limit.unwrap_or(50).min(100)).min(self.badges.len());
        (from..to).filter_map(|i| self.badges.get(i).map(|b| self.view_badge(i, b))).collect()
    }

    pub fn nft_supply_for_owner(&self, account_id: AccountId) -> U128 {
        U128(self.accounts.get(&account_id).map(|h| h.badges.len()).unwrap_or(0) as u128)
    }

    pub fn nft_tokens_for_owner(
        &self,
        account_id: AccountId,
        from_index: Option<U128>,
        limit: Option<u32>,
    ) -> Vec<NftToken> {
        let Some(h) = self.accounts.get(&account_id) else { return vec![] };
        h.badges
            .iter()
            .skip(from_index.map(|i| i.0 as usize).unwrap_or(0))
            .take(limit.unwrap_or(50) as usize)
            .filter_map(|i| self.badges.get(*i).map(|b| self.view_badge(*i, b)))
            .collect()
    }

    /// Badges are earned, not traded.
    #[payable]
    pub fn nft_transfer(
        &mut self,
        receiver_id: AccountId,
        token_id: String,
        approval_id: Option<u64>,
        memo: Option<String>,
    ) {
        let _ = (receiver_id, token_id, approval_id, memo);
        env::panic_str("badges are soulbound");
    }

    #[payable]
    pub fn nft_transfer_call(
        &mut self,
        receiver_id: AccountId,
        token_id: String,
        approval_id: Option<u64>,
        memo: Option<String>,
        msg: String,
    ) -> PromiseOrValue<bool> {
        let _ = (receiver_id, token_id, approval_id, memo, msg);
        env::panic_str("badges are soulbound");
    }
}

impl Contract {
    /// Settle every raid before today, in order (at most 20 per call), then open today's raid with its target.
    pub(crate) fn roll_raids(&mut self) {
        let today = self.day_index();
        let mut n = 0;
        while self.raids_settled < today && n < MAX_SETTLE_PER_CALL {
            let id = self.raids_settled;
            let mut raid = self.raids.get(&id).cloned().unwrap_or_default();
            let won = raid.joiners >= self.config.raid.min_joiners && raid.joined.0 >= raid.target.0 && raid.joined.0 > 0;
            if won {
                self.streak += 1;
                self.best_streak = self.best_streak.max(self.streak);
                self.raids_won += 1;
            } else {
                self.streak = 0;
                self.raids_lost += 1;
                let spend = bps(self.chest, self.config.raid.fail_burn_bps);
                if spend > 0 {
                    let out = mul_div(self.token_reserve, spend, self.near_reserve + spend);
                    self.chest -= spend;
                    self.near_reserve += spend;
                    self.token_reserve -= out;
                    self.burned += out;
                    raid.burned = U128(out);
                    log_ft("ft_burn", json!([{ "owner_id": env::current_account_id(), "amount": U128(out), "memo": "raid lost" }]));
                }
            }
            raid.won = Some(won);
            raid.streak = self.streak;
            // raids nobody joined and that burned nothing leave no trace
            if raid.joiners > 0 || raid.burned.0 > 0 {
                self.raids.insert(id, raid.clone());
                let caller = env::predecessor_account_id();
                self.record(if won { "raid_won" } else { "raid_lost" }, &caller, 0, raid.burned.0);
            } else {
                self.raids.remove(&id);
            }
            self.raids_settled += 1;
            n += 1;
        }
        if self.raids_settled == today && !self.raids.contains_key(&today) {
            let raid = self.open_raid(today);
            self.raids.insert(today, raid);
        }
    }

    /// Today's raid; its target is set from the circulating supply the first time it is touched.
    fn open_raid(&self, id: u64) -> RaidState {
        self.raids.get(&id).cloned().unwrap_or(RaidState {
            target: U128(bps(self.circulating, self.config.raid.target_bps)),
            ..Default::default()
        })
    }

    fn claim_for(&mut self, account: &AccountId) -> ClaimView {
        let holder = self.accounts.get(account).expect("account not registered");
        let joined = holder.joined.clone().expect("no raid to claim");
        let id = joined.raid.0;
        require!(id < self.raids_settled, "the raid is not over yet");
        let raid = self.raids.get(&id).cloned().expect("raid not found");
        self.accounts.get_mut(account).unwrap().joined = None;
        let mut view = ClaimView { raid: U64(id), won: raid.won == Some(true), minted: U128(0), pulls: vec![], badges: vec![] };
        if !view.won {
            return view;
        }
        // tokens sold since joining are not paid for
        let amount = joined.amount.0.min(self.balance(account));
        let bonus = 10_000 + 1_000 * raid.streak.min(10) as u128;
        let minted = mul_div(bps(amount, self.config.raid.mint_bps), bonus, 10_000);
        if minted > 0 {
            self.credit(account, minted);
            self.circulating += minted;
            self.minted += minted;
            log_ft("ft_mint", json!([{ "owner_id": account, "amount": U128(minted), "memo": "raid" }]));
        }
        view.minted = U128(minted);
        // this raider's share of the raid's pulls: whole pulls, and the fraction decided by a draw
        let share = mul_div(self.config.raid.raid_pulls as u128 * PULL_SCALE, amount, raid.joined.0.max(1));
        let mut pulls = (share / PULL_SCALE) as u32;
        if (self.random(account) as u128 % PULL_SCALE) < share % PULL_SCALE {
            pulls += 1;
        }
        let mut won_near = 0;
        for _ in 0..pulls.min(MAX_PULLS_PER_CLAIM) {
            let p = self.pull(account, &mut view.badges);
            won_near += p.near.0;
            view.pulls.push(p);
        }
        if self.award(account, FIRST_BLOOD) {
            view.badges.push(BADGES[FIRST_BLOOD as usize].0.into());
        }
        if raid.streak >= 7 && self.award(account, SEVEN_DAYS) {
            view.badges.push(BADGES[SEVEN_DAYS as usize].0.into());
        }
        if won_near > 0 {
            Promise::new(account.clone()).transfer(NearToken::from_yoctonear(won_near)).detach();
        }
        self.record("raid_claim", account, won_near, minted);
        view
    }

    /// One pull from the chest. The NEAR is not sent here: callers add up and transfer once.
    pub(crate) fn pull(&mut self, account: &AccountId, badges: &mut Vec<String>) -> PullView {
        let o = odds(self.odds_level(account));
        let mut x = (self.random(account) % 10_000) as u32;
        let mut tier = 0;
        while x >= o[tier] {
            x -= o[tier];
            tier += 1;
        }
        let win = bps(self.chest, TIERS[tier].2);
        self.chest -= win;
        if tier == 3 && self.award(account, LEGENDARY) {
            badges.push(BADGES[LEGENDARY as usize].0.into());
        }
        log_event("pull", json!({ "account_id": account, "tier": TIERS[tier].0, "near": U128(win) }));
        // the big ones go to the public log; commons and rares are summed in the raid_claim line
        if tier >= 2 {
            self.store_entry(&format!("loot_{}", TIERS[tier].0), account, win, 0);
        }
        PullView { tier: TIERS[tier].0.into(), near: U128(win) }
    }

    /// Mint a soulbound badge once per kind. Returns false if the account already has it.
    fn award(&mut self, account: &AccountId, kind: u8) -> bool {
        let owned = {
            let h = self.accounts.get(account).expect("account not registered");
            h.badges.iter().any(|i| self.badges.get(*i).map(|b| b.kind) == Some(kind))
        };
        if owned {
            return false;
        }
        let index = self.badges.len();
        self.badges.push(Badge { owner_id: account.clone(), kind, issued_at: U64(env::block_timestamp()) });
        self.accounts.get_mut(account).unwrap().badges.push(index);
        env::log_str(&format!(
            "EVENT_JSON:{}",
            json!({ "standard": "nep171", "version": "1.2.0", "event": "nft_mint",
                    "data": [{ "owner_id": account, "token_ids": [index.to_string()], "memo": BADGES[kind as usize].0 }] })
        ));
        true
    }

    fn view_badge(&self, i: u32, b: &Badge) -> NftToken {
        let (title, description) = BADGES[b.kind as usize];
        NftToken {
            token_id: i.to_string(),
            owner_id: b.owner_id.clone(),
            metadata: TokenMetadata {
                title: Some(title.into()),
                description: Some(description.into()),
                media: None,
                issued_at: Some((b.issued_at.0 / 1_000_000).to_string()),
            },
        }
    }

    /// Level for the loot odds: the balance-weighted level of the account's lots (weight / balance, rounded down)
    /// plus two per badge, capped at 10. A dust lot kept from an old bag does not keep the old bag's odds.
    fn odds_level(&self, account: &AccountId) -> u32 {
        let Some(h) = self.accounts.get(account) else { return 0 };
        let balance = total(&h.lots);
        let level = if balance == 0 { 0 } else { (self.weight(&h.lots) / balance) as u32 };
        (level + 2 * h.badges.len() as u32).min(MAX_ODDS_LEVEL)
    }

    /// Balance in lots held at least one day.
    pub(crate) fn seasoned(&self, lots: &[Lot]) -> u128 {
        lots.iter().filter(|l| self.level(l.since.0) >= 1).map(|l| l.amount.0).sum()
    }

    /// Tokens an extra pull costs: ticket_cost_bps of the chest, at min(spot, yesterday's high).
    fn ticket_tokens(&self) -> u128 {
        let near = bps(self.chest, self.config.raid.ticket_cost_bps);
        let spot = self.price();
        let today = self.day_index();
        let yesterday = if self.high_day == today {
            self.high_prev
        } else if self.high_day + 1 == today {
            self.high_today
        } else {
            0
        };
        let reference = if yesterday > 0 { spot.min(yesterday) } else { spot };
        if near == 0 || reference == 0 {
            return 0;
        }
        mul_div(near, ONE_TOKEN, reference)
    }

    fn random(&mut self, account: &AccountId) -> u64 {
        let mut data = env::random_seed();
        data.extend_from_slice(account.as_bytes());
        data.extend_from_slice(&self.pull_nonce.to_le_bytes());
        self.pull_nonce += 1;
        let h = env::sha256(&data);
        u64::from_le_bytes(h[0..8].try_into().unwrap())
    }
}

fn log_event(event: &str, data: serde_json::Value) {
    env::log_str(&format!(
        "EVENT_JSON:{}",
        json!({ "standard": "warband", "version": "1.0.0", "event": event, "data": [data] })
    ));
}
