//! WARBAND bounty board with earned trust, on top of the band's Sputnik DAO.
//!
//! Members post bounties, contributors claim and finish them, the poster accepts the work, and the payout goes through
//! one of three tiers, decided by the contributor's record on this board:
//!
//! - **auto**: the amount is within the contributor's trust limit (`trust_step` per bounty of at least `trust_step`
//!   that the council or the DAO approved for them, capped at `auto_cap`): paid at once from the petty cash the DAO
//!   voted to this contract. Auto payouts do not raise trust, and one contributor receives at most `auto_cap` in auto
//!   payouts per 24 hours, so a poster and a contributor who collude cannot farm trust or drain the petty cash.
//! - **council**: up to `council_cap`: paid from the petty cash once one member of the DAO's `council` role approves.
//!   Membership is read from the DAO's live policy (`get_policy`) at approval time.
//! - **dao**: above `council_cap`: the contract files a normal `Transfer` proposal in the DAO (the poster pays the
//!   proposal bond); the DAO's own vote pays the contributor; `settle_dao` reads the outcome.
//!
//! Custody and approvals stay in the DAO: the only money here is petty cash that a DAO `Transfer` proposal sent, and
//! the tiers can only be changed by the DAO itself (`set_tiers` from the DAO account). No owner, no admin method.
//! Reputation is per named account (implicit accounts cannot claim) and every point is a paid bounty listed in
//! `record_of`.
use near_sdk::json_types::{U128, U64};
use near_sdk::serde_json::{self, json, Value};
use near_sdk::store::{LookupMap, Vector};
use near_sdk::{env, near, require, AccountId, Gas, NearToken, PanicOnDefault, Promise, PromiseError};

const POST_DEPOSIT: u128 = 10_000_000_000_000_000_000_000; // 0.01 NEAR: storage of one bounty, kept
const RESERVE: u128 = 50_000_000_000_000_000_000_000; // 0.05 NEAR never paid out (storage headroom)
const GAS_VIEW: Gas = Gas::from_tgas(10);
const GAS_CALLBACK: Gas = Gas::from_tgas(20);
const GAS_PROPOSAL: Gas = Gas::from_tgas(30);
const GAS_BOND_CALLBACK: Gas = Gas::from_tgas(70);
const DAY_MS: u64 = 86_400_000;

/// Implicit accounts (64 hex characters, or an Ethereum-style 0x address) carry no name, so no reputation.
fn is_named(account: &AccountId) -> bool {
    let s = account.as_str();
    let hex64 = s.len() == 64 && s.bytes().all(|c| c.is_ascii_hexdigit());
    let eth = s.len() == 42 && s.starts_with("0x") && s[2..].bytes().all(|c| c.is_ascii_hexdigit());
    !hex64 && !eth
}

#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Open,
    Claimed,
    Submitted,
    AwaitingCouncil,
    Proposed,
    Paid,
    Rejected,
    Cancelled,
}

#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub enum Tier {
    Auto,
    Council,
    Dao,
}

#[near(serializers = [borsh, json])]
#[derive(Clone)]
pub struct Bounty {
    pub id: U64,
    pub poster: AccountId,
    pub title: String,
    pub amount: U128,
    pub status: Status,
    pub contributor: Option<AccountId>,
    pub proof: Option<String>,
    pub tier: Option<Tier>,
    pub approved_by: Option<AccountId>,
    pub dao_proposal: Option<U64>,
    /// the DAO proposal bond the poster paid, passed back to them when the DAO returns it
    pub bond: Option<U128>,
    pub posted_at_ms: U64,
    pub paid_at_ms: Option<U64>,
}

/// One reputation point: a bounty paid to this account, how, and who approved it.
#[near(serializers = [borsh, json])]
#[derive(Clone)]
pub struct Point {
    pub bounty: U64,
    pub amount: U128,
    pub tier: Tier,
    pub approved_by: AccountId,
    pub at_ms: U64,
}

#[near(serializers = [borsh, json])]
#[derive(Clone)]
pub struct Tiers {
    /// trust limit added by each bounty already paid to a contributor
    pub trust_step: U128,
    /// the trust limit never goes above this
    pub auto_cap: U128,
    /// above this, a full DAO vote
    pub council_cap: U128,
}

#[near(serializers = [json])]
pub struct RecordView {
    pub account_id: AccountId,
    pub finished: u32,
    pub total_paid: U128,
    pub trust_limit: U128,
    pub points: Vec<Point>,
}

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct Bounties {
    dao: AccountId,
    tiers: Tiers,
    bounties: Vector<Bounty>,
    records: LookupMap<AccountId, Vec<Point>>,
}

#[near]
impl Bounties {
    #[init]
    pub fn new(dao: AccountId, tiers: Tiers) -> Self {
        Self::check_tiers(&tiers);
        Self { dao, tiers, bounties: Vector::new(b"b"), records: LookupMap::new(b"r") }
    }

    // ---- the bounty flow ----

    /// Any account posts a bounty (0.01 NEAR deposit, kept for storage). Posting does not move treasury money.
    #[payable]
    pub fn post(&mut self, title: String, amount: U128) -> U64 {
        require!(env::attached_deposit().as_yoctonear() >= POST_DEPOSIT, "Attach 0.01 NEAR for storage");
        require!(!title.trim().is_empty() && title.len() <= 140, "Title: 1 to 140 characters");
        require!(amount.0 > 0, "Amount must be positive");
        let id = self.bounties.len() as u64;
        let b = Bounty {
            id: U64(id),
            poster: env::predecessor_account_id(),
            title,
            amount,
            status: Status::Open,
            contributor: None,
            proof: None,
            tier: None,
            approved_by: None,
            dao_proposal: None,
            bond: None,
            posted_at_ms: U64(env::block_timestamp_ms()),
            paid_at_ms: None,
        };
        self.bounties.push(b);
        self.log("post", id, json!({ "poster": env::predecessor_account_id(), "amount": amount }));
        U64(id)
    }

    pub fn claim(&mut self, id: U64) {
        let who = env::predecessor_account_id();
        let b = self.get_mut(id);
        require!(b.status == Status::Open, "Bounty is not open");
        require!(b.poster != who, "The poster cannot claim their own bounty");
        require!(is_named(&who), "Reputation needs a named account: implicit accounts cannot claim");
        b.status = Status::Claimed;
        b.contributor = Some(who.clone());
        self.log("claim", id.0, json!({ "contributor": who }));
    }

    pub fn submit(&mut self, id: U64, proof: String) {
        let who = env::predecessor_account_id();
        require!(!proof.trim().is_empty() && proof.len() <= 200, "Proof: 1 to 200 characters (a link, a hash)");
        let b = self.get_mut(id);
        require!(b.status == Status::Claimed, "Bounty is not claimed");
        require!(b.contributor.as_ref() == Some(&who), "Only the contributor who claimed it can submit");
        b.status = Status::Submitted;
        b.proof = Some(proof);
        self.log("submit", id.0, json!({ "contributor": who }));
    }

    /// The poster accepts the work; the tier decides what happens next. For the `dao` tier, attach the DAO's proposal
    /// bond (it goes to the DAO with the proposal and comes back to the poster under the DAO's rules).
    #[payable]
    pub fn accept(&mut self, id: U64) -> Option<Promise> {
        let who = env::predecessor_account_id();
        let (contributor, amount) = {
            let b = self.get(id);
            require!(b.status == Status::Submitted, "Nothing submitted to accept");
            require!(b.poster == who, "Only the poster accepts the work");
            (b.contributor.clone().unwrap(), b.amount.0)
        };
        let tier = self.tier_for(&contributor, amount);
        match tier {
            Tier::Auto => {
                self.pay(id, Tier::Auto, who);
                None
            }
            Tier::Council => {
                let b = self.get_mut(id);
                b.status = Status::AwaitingCouncil;
                b.tier = Some(Tier::Council);
                self.log("awaiting_council", id.0, json!({}));
                None
            }
            Tier::Dao => {
                let attached = env::attached_deposit();
                require!(attached.as_yoctonear() > 0, "A DAO proposal: attach the DAO's proposal bond");
                // marked now so the bounty cannot be accepted twice while the DAO's bond is read
                let b = self.get_mut(id);
                b.status = Status::Proposed;
                b.tier = Some(Tier::Dao);
                // the board files the proposal with exactly the DAO's `proposal_bond` (Sputnik locks whatever is
                // attached but returns only that), and gives any surplus back to the poster at once
                Some(
                    Promise::new(self.dao.clone())
                        .function_call("get_policy", b"{}".to_vec(), NearToken::from_yoctonear(0), GAS_VIEW)
                        .then(Self::ext(env::current_account_id()).with_static_gas(GAS_BOND_CALLBACK).on_bond(id, who, U128(attached.as_yoctonear()))),
                )
            }
        }
    }

    /// One member of the DAO's council approves a mid-size payout. Membership is read from the DAO's policy now.
    pub fn approve(&mut self, id: U64) -> Promise {
        let who = env::predecessor_account_id();
        let b = self.get(id);
        require!(b.status == Status::AwaitingCouncil, "Bounty is not waiting for the council");
        require!(b.contributor.as_ref() != Some(&who), "The contributor cannot approve their own payout");
        require!(b.poster != who, "The poster cannot approve their own bounty: another council member must");
        Promise::new(self.dao.clone())
            .function_call("get_policy", b"{}".to_vec(), NearToken::from_yoctonear(0), GAS_VIEW)
            .then(Self::ext(env::current_account_id()).with_static_gas(GAS_CALLBACK).on_policy(id, who))
    }

    /// Reads the DAO's decision on a `dao`-tier bounty. Anyone can call it.
    pub fn settle_dao(&mut self, id: U64) -> Promise {
        let b = self.get(id);
        require!(b.status == Status::Proposed, "Bounty has no DAO proposal pending");
        let pid = b.dao_proposal.unwrap_or_else(|| env::panic_str("Proposal id not recorded yet"));
        Promise::new(self.dao.clone())
            .function_call("get_proposal", json!({ "id": pid.0 }).to_string().into_bytes(), NearToken::from_yoctonear(0), GAS_VIEW)
            .then(Self::ext(env::current_account_id()).with_static_gas(GAS_CALLBACK).on_proposal_status(id))
    }

    /// The poster withdraws a bounty nobody has submitted work for yet.
    pub fn cancel(&mut self, id: U64) {
        let who = env::predecessor_account_id();
        let b = self.get_mut(id);
        require!(b.poster == who, "Only the poster can cancel");
        require!(b.status == Status::Open || b.status == Status::Claimed, "Too late to cancel");
        b.status = Status::Cancelled;
        self.log("cancel", id.0, json!({}));
    }

    /// Tiers change only by a DAO vote (a FunctionCall proposal from the DAO account).
    pub fn set_tiers(&mut self, tiers: Tiers) {
        require!(env::predecessor_account_id() == self.dao, "Only the DAO changes the tiers");
        Self::check_tiers(&tiers);
        self.tiers = tiers;
        env::log_str(&format!("EVENT_JSON:{}", json!({ "standard": "warband_bounties", "version": "1.0.0", "event": "set_tiers", "data": [self.tiers] })));
    }

    /// The DAO takes its petty cash back (all of it, or `amount`), by a DAO vote.
    pub fn return_petty_cash(&mut self, amount: Option<U128>) -> Promise {
        require!(env::predecessor_account_id() == self.dao, "Only the DAO takes the petty cash back");
        let cash = self.petty_cash();
        let amount = amount.map(|a| a.0).unwrap_or(cash);
        require!(amount <= cash, "More than the petty cash");
        env::log_str(&format!("EVENT_JSON:{}", json!({ "standard": "warband_bounties", "version": "1.0.0", "event": "return_petty_cash", "data": [{ "amount": U128(amount) }] })));
        Promise::new(self.dao.clone()).transfer(NearToken::from_yoctonear(amount))
    }

    // ---- callbacks ----

    #[private]
    pub fn on_policy(&mut self, id: U64, approver: AccountId, #[callback_result] policy: Result<Value, PromiseError>) -> bool {
        let policy = policy.unwrap_or_else(|_| env::panic_str("Could not read the DAO policy"));
        require!(Self::is_council(&policy, &approver), "Not a member of the DAO council");
        require!(self.get(id).poster != approver, "The poster cannot approve their own bounty: another council member must");
        require!(self.get(id).status == Status::AwaitingCouncil, "Bounty is not waiting for the council");
        self.pay(id, Tier::Council, approver);
        true
    }

    #[private]
    pub fn on_bond(&mut self, id: U64, poster: AccountId, attached: U128, #[callback_result] policy: Result<Value, PromiseError>) -> Option<Promise> {
        let need = policy.ok().and_then(|p| p["proposal_bond"].as_str().and_then(|s| s.parse::<u128>().ok()));
        let Some(need) = need.filter(|n| attached.0 >= *n) else {
            // the bond could not be read, or too little was attached: back to Submitted, everything back to the poster
            let b = self.get_mut(id);
            b.status = Status::Submitted;
            b.tier = None;
            Promise::new(poster).transfer(NearToken::from_yoctonear(attached.0)).detach();
            self.log("proposal_failed", id.0, json!({ "reason": "attach at least the DAO's proposal bond" }));
            return None;
        };
        if attached.0 > need {
            Promise::new(poster.clone()).transfer(NearToken::from_yoctonear(attached.0 - need)).detach();
        }
        let (title, contributor, amount) = {
            let b = self.get(id);
            (b.title.clone(), b.contributor.clone().unwrap(), b.amount)
        };
        let proposal = json!({ "proposal": {
            "description": format!("Warband bounty #{}: {} (accepted by {}, board {})", id.0, title, poster, env::current_account_id()),
            "kind": { "Transfer": { "token_id": "", "receiver_id": contributor, "amount": amount } }
        } });
        Some(
            Promise::new(self.dao.clone())
                .function_call("add_proposal", proposal.to_string().into_bytes(), NearToken::from_yoctonear(need), GAS_PROPOSAL)
                .then(Self::ext(env::current_account_id()).with_static_gas(GAS_CALLBACK).on_proposed(id, poster, U128(need))),
        )
    }

    #[private]
    pub fn on_proposed(&mut self, id: U64, poster: AccountId, bond: U128, #[callback_result] pid: Result<u64, PromiseError>) -> Option<U64> {
        match pid {
            Ok(pid) => {
                let b = self.get_mut(id);
                b.dao_proposal = Some(U64(pid));
                b.bond = Some(bond);
                self.log("proposed", id.0, json!({ "dao_proposal": pid }));
                Some(U64(pid))
            }
            Err(_) => {
                // the DAO refused the proposal (bond, policy): back to Submitted, the bond back to the poster
                let b = self.get_mut(id);
                b.status = Status::Submitted;
                b.tier = None;
                Promise::new(poster).transfer(NearToken::from_yoctonear(bond.0)).detach();
                self.log("proposal_failed", id.0, json!({}));
                None
            }
        }
    }

    #[private]
    pub fn on_proposal_status(&mut self, id: U64, #[callback_result] proposal: Result<Value, PromiseError>) -> String {
        // Sputnik deletes a proposal once it is removed: `get_proposal` then fails, and the bounty is closed as rejected
        let status = match proposal {
            Ok(p) => p["status"].as_str().unwrap_or("").to_string(),
            Err(_) => "Removed".to_string(),
        };
        require!(self.get(id).status == Status::Proposed, "Bounty has no DAO proposal pending");
        match status.as_str() {
            "Approved" => {
                // the DAO already paid the contributor itself; record the point
                let dao = self.dao.clone();
                self.record(id, Tier::Dao, dao);
            }
            // `Failed` (approved, but the transfer failed) can still be finalized again in the DAO: not final here
            "Rejected" | "Removed" | "Expired" => {
                let b = self.get_mut(id);
                b.status = Status::Rejected;
                self.log("rejected", id.0, json!({ "dao_status": status }));
            }
            _ => env::panic_str(&format!("The DAO has not decided yet ({status})")),
        }
        // Sputnik returns the proposal bond to the proposer (this board) when a proposal is approved, rejected or
        // expires, and keeps it when the proposal is removed: pass it on to the poster who paid it
        if matches!(status.as_str(), "Approved" | "Rejected" | "Expired") {
            let (poster, bond) = {
                let b = self.get_mut(id);
                (b.poster.clone(), b.bond.take())
            };
            if let Some(bond) = bond.filter(|b| b.0 > 0) {
                Promise::new(poster).transfer(NearToken::from_yoctonear(bond.0)).detach();
            }
        }
        status
    }

    // ---- views ----

    pub fn bounties(&self, from_index: Option<u32>, limit: Option<u32>) -> Vec<Bounty> {
        let from = from_index.unwrap_or(0);
        self.bounties.iter().skip(from as usize).take(limit.unwrap_or(50).min(100) as usize).cloned().collect()
    }

    pub fn bounty(&self, id: U64) -> Option<Bounty> {
        self.bounties.get(id.0 as u32).cloned()
    }

    pub fn record_of(&self, account_id: AccountId) -> RecordView {
        let points = self.records.get(&account_id).cloned().unwrap_or_default();
        RecordView {
            finished: points.len() as u32,
            total_paid: U128(points.iter().map(|p| p.amount.0).sum()),
            trust_limit: U128(self.trust_limit(&account_id)),
            points,
            account_id,
        }
    }

    pub fn tier_of(&self, account_id: AccountId, amount: U128) -> Tier {
        self.tier_for(&account_id, amount.0)
    }

    pub fn config(&self) -> Value {
        json!({ "dao": self.dao, "tiers": self.tiers, "petty_cash": U128(self.petty_cash()) })
    }

    // ---- internals ----

    /// Trust comes only from work someone other than the poster checked: a bounty of at least `trust_step` approved
    /// by a council member or by a DAO vote. Auto payouts are recorded but do not raise it.
    fn trust_limit(&self, account: &AccountId) -> u128 {
        let step = self.tiers.trust_step.0;
        let earned = self.records.get(account).map(|ps| ps.iter().filter(|p| p.tier != Tier::Auto && p.amount.0 >= step).count()).unwrap_or(0) as u128;
        (step * earned).min(self.tiers.auto_cap.0)
    }

    /// Auto payouts this contributor received in the last 24 hours.
    fn auto_paid_today(&self, account: &AccountId) -> u128 {
        let now = env::block_timestamp_ms();
        self.records.get(account).map(|ps| ps.iter().filter(|p| p.tier == Tier::Auto && now.saturating_sub(p.at_ms.0) < DAY_MS).map(|p| p.amount.0).sum()).unwrap_or(0)
    }

    fn tier_for(&self, contributor: &AccountId, amount: u128) -> Tier {
        if amount <= self.trust_limit(contributor) && self.auto_paid_today(contributor) + amount <= self.tiers.auto_cap.0 {
            Tier::Auto
        } else if amount <= self.tiers.council_cap.0 {
            Tier::Council
        } else {
            Tier::Dao
        }
    }

    fn petty_cash(&self) -> u128 {
        let storage = env::storage_byte_cost().as_yoctonear() * env::storage_usage() as u128;
        env::account_balance().as_yoctonear().saturating_sub(storage + RESERVE)
    }

    fn pay(&mut self, id: U64, tier: Tier, approver: AccountId) {
        let (to, amount) = {
            let b = self.get(id);
            (b.contributor.clone().unwrap(), b.amount.0)
        };
        require!(amount <= self.petty_cash(), "Petty cash too low: the DAO must vote more to this board");
        self.record(id, tier, approver);
        Promise::new(to).transfer(NearToken::from_yoctonear(amount)).detach();
    }

    fn record(&mut self, id: U64, tier: Tier, approver: AccountId) {
        let now = U64(env::block_timestamp_ms());
        let (to, amount) = {
            let b = self.get_mut(id);
            b.status = Status::Paid;
            b.tier = Some(tier.clone());
            b.approved_by = Some(approver.clone());
            b.paid_at_ms = Some(now);
            (b.contributor.clone().unwrap(), b.amount)
        };
        let mut points = self.records.get(&to).cloned().unwrap_or_default();
        points.push(Point { bounty: id, amount, tier: tier.clone(), approved_by: approver.clone(), at_ms: now });
        self.records.insert(to.clone(), points);
        self.log("paid", id.0, json!({ "contributor": to, "amount": amount, "tier": tier, "approved_by": approver }));
    }

    fn is_council(policy: &Value, who: &AccountId) -> bool {
        policy["roles"].as_array().into_iter().flatten().any(|role| {
            role["name"] == "council"
                && role["kind"]["Group"].as_array().into_iter().flatten().any(|m| m.as_str() == Some(who.as_str()))
        })
    }

    fn check_tiers(t: &Tiers) {
        require!(t.trust_step.0 > 0 && t.auto_cap.0 >= t.trust_step.0, "auto_cap must be at least one trust_step");
        require!(t.council_cap.0 >= t.auto_cap.0, "council_cap must be at least auto_cap");
    }

    fn get(&self, id: U64) -> &Bounty {
        self.bounties.get(id.0 as u32).unwrap_or_else(|| env::panic_str("No such bounty"))
    }

    fn get_mut(&mut self, id: U64) -> &mut Bounty {
        self.bounties.get_mut(id.0 as u32).unwrap_or_else(|| env::panic_str("No such bounty"))
    }

    fn log(&self, event: &str, id: u64, data: Value) {
        let mut data = data;
        data["bounty"] = json!(id.to_string());
        env::log_str(&format!("EVENT_JSON:{}", serde_json::to_string(&json!({ "standard": "warband_bounties", "version": "1.0.0", "event": event, "data": [data] })).unwrap()));
    }
}

#[cfg(test)]
mod tests;
