use super::*;
use near_sdk::test_utils::{get_logs, VMContextBuilder};
use near_sdk::testing_env;

const NEAR: u128 = 1_000_000_000_000_000_000_000_000;
fn acc(s: &str) -> AccountId {
    s.parse().unwrap()
}
fn ctx(who: &str, deposit: u128) {
    testing_env!(VMContextBuilder::new()
        .current_account_id(acc("board.testnet"))
        .predecessor_account_id(acc(who))
        .attached_deposit(NearToken::from_yoctonear(deposit))
        .account_balance(NearToken::from_near(5))
        .build());
}
fn tiers() -> Tiers {
    // trust 0.05 NEAR per paid bounty, auto up to 0.25, council up to 0.5, above: DAO vote
    Tiers { trust_step: U128(NEAR / 20), auto_cap: U128(NEAR / 4), council_cap: U128(NEAR / 2) }
}
fn board() -> Bounties {
    ctx("hq.testnet", 0);
    Bounties::new(acc("dao.testnet"), tiers())
}
fn policy(council: &[&str]) -> Value {
    json!({ "roles": [
        { "name": "everyone", "kind": "Everyone", "permissions": ["*:AddProposal"] },
        { "name": "council", "kind": { "Group": council }, "permissions": ["vote:*"] },
        { "name": "holders", "kind": { "Member": "1" }, "permissions": ["transfer:*"] }
    ] })
}
/// posted by alice, claimed and submitted by dave
fn submitted(c: &mut Bounties, amount: u128) -> U64 {
    ctx("alice.testnet", POST_DEPOSIT);
    let id = c.post("Draw the Warlord badge".into(), U128(amount));
    ctx("dave.testnet", 0);
    c.claim(id);
    c.submit(id, "ipfs://badge".into());
    id
}
fn council_pays(c: &mut Bounties, id: U64) {
    ctx("alice.testnet", 0);
    assert!(c.accept(id).is_none());
    assert_eq!(c.bounty(id).unwrap().status, Status::AwaitingCouncil);
    ctx("board.testnet", 0);
    assert!(c.on_policy(id, acc("hq.testnet"), Ok(policy(&["hq.testnet"]))));
}

#[test]
fn a_newcomer_needs_the_council_then_earns_auto_payouts() {
    let mut c = board();
    assert_eq!(c.tier_of(acc("dave.testnet"), U128(NEAR / 100)), Tier::Council, "no record, no trust");
    let first = submitted(&mut c, NEAR / 20);
    council_pays(&mut c, first);
    let r = c.record_of(acc("dave.testnet"));
    assert_eq!((r.finished, r.trust_limit.0), (1, NEAR / 20));
    assert_eq!(r.points[0].approved_by, acc("hq.testnet"));

    // the second one, within the trust limit, is paid when alice accepts it
    let second = submitted(&mut c, NEAR / 25);
    ctx("alice.testnet", 0);
    assert!(c.accept(second).is_none());
    let b = c.bounty(second).unwrap();
    assert_eq!((b.status, b.tier, b.approved_by), (Status::Paid, Some(Tier::Auto), Some(acc("alice.testnet"))));
    assert!(get_logs().iter().any(|l| l.contains("\"event\":\"paid\"") && l.contains("Auto")));
    assert_eq!(c.record_of(acc("dave.testnet")).finished, 2);
}

#[test]
fn trust_is_capped() {
    let mut c = board();
    // eight council-approved bounties above the auto cap: each adds one trust step, up to the cap
    for _ in 0..8 {
        let id = submitted(&mut c, NEAR * 3 / 10);
        council_pays(&mut c, id);
    }
    assert_eq!(c.record_of(acc("dave.testnet")).trust_limit.0, NEAR / 4);
    assert_eq!(c.tier_of(acc("dave.testnet"), U128(NEAR / 4 + 1)), Tier::Council);
}

/// alice and dave collude: after one council approval, alice posts tiny bounties for dave and accepts them herself
#[test]
fn auto_payouts_earn_no_trust_and_cannot_be_farmed() {
    let mut c = board();
    let first = submitted(&mut c, NEAR / 20);
    council_pays(&mut c, first);
    for _ in 0..10 {
        let id = submitted(&mut c, 1);
        ctx("alice.testnet", 0);
        c.accept(id);
        assert_eq!(c.bounty(id).unwrap().tier, Some(Tier::Auto));
    }
    let r = c.record_of(acc("dave.testnet"));
    assert_eq!((r.finished, r.trust_limit.0), (11, NEAR / 20), "ten auto payouts, still one trust step");
}

#[test]
fn a_bounty_under_one_trust_step_earns_no_trust_even_with_the_council() {
    let mut c = board();
    let id = submitted(&mut c, 1);
    council_pays(&mut c, id);
    assert_eq!(c.record_of(acc("dave.testnet")).trust_limit.0, 0);
}

#[test]
fn auto_payouts_to_one_contributor_are_capped_per_day() {
    let mut c = board();
    for _ in 0..5 {
        let id = submitted(&mut c, NEAR * 3 / 10);
        council_pays(&mut c, id);
    }
    assert_eq!(c.record_of(acc("dave.testnet")).trust_limit.0, NEAR / 4);
    let a = submitted(&mut c, NEAR / 5);
    ctx("alice.testnet", 0);
    c.accept(a);
    assert_eq!(c.bounty(a).unwrap().tier, Some(Tier::Auto));
    // 0.2 already today: another 0.1 would pass the 0.25 daily cap, so it waits for the council
    assert_eq!(c.tier_of(acc("dave.testnet"), U128(NEAR / 10)), Tier::Council);
    // the next day, auto again
    let later = near_sdk::env::block_timestamp() + (DAY_MS + 1) * 1_000_000;
    testing_env!(VMContextBuilder::new()
        .current_account_id(acc("board.testnet"))
        .predecessor_account_id(acc("alice.testnet"))
        .block_timestamp(later)
        .account_balance(NearToken::from_near(5))
        .build());
    assert_eq!(c.tier_of(acc("dave.testnet"), U128(NEAR / 10)), Tier::Auto);
}

#[test]
#[should_panic(expected = "implicit accounts cannot claim")]
fn an_implicit_account_cannot_claim() {
    let mut c = board();
    ctx("alice.testnet", POST_DEPOSIT);
    let id = c.post("Draw".into(), U128(NEAR / 20));
    ctx("98793cd91a3f870fb126f66285808c7e094afcfc4eda8a970f6648cdf0dbd6de", 0);
    c.claim(id);
}

#[test]
fn a_removed_proposal_closes_the_bounty_and_a_failed_one_stays_pending() {
    let mut c = board();
    let id = submitted(&mut c, NEAR);
    ctx("alice.testnet", NEAR / 10);
    c.accept(id);
    ctx("board.testnet", 0);
    c.on_proposed(id, acc("alice.testnet"), U128(NEAR / 10), Ok(9));
    let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut c2 = board();
        let id = submitted(&mut c2, NEAR);
        ctx("alice.testnet", NEAR / 10);
        c2.accept(id);
        ctx("board.testnet", 0);
        c2.on_proposed(id, acc("alice.testnet"), U128(NEAR / 10), Ok(9));
        c2.on_proposal_status(id, Ok(json!({ "status": "Failed" })));
    }));
    assert!(failed.is_err(), "Failed can be finalized again in the DAO");
    assert_eq!(c.on_proposal_status(id, Err(PromiseError::Failed)), "Removed");
    assert_eq!(c.bounty(id).unwrap().status, Status::Rejected);
}

#[test]
fn large_payouts_become_a_dao_proposal() {
    let mut c = board();
    let id = submitted(&mut c, NEAR);
    ctx("alice.testnet", NEAR / 10);
    assert!(c.accept(id).is_some(), "add_proposal promise");
    assert_eq!(c.bounty(id).unwrap().status, Status::Proposed);
    ctx("board.testnet", 0);
    assert_eq!(c.on_proposed(id, acc("alice.testnet"), U128(NEAR / 10), Ok(7)), Some(U64(7)));
    assert_eq!(c.bounty(id).unwrap().dao_proposal, Some(U64(7)));
    // still in progress: settle refuses
    let pending = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut c2 = board();
        let id = submitted(&mut c2, NEAR);
        ctx("alice.testnet", NEAR / 10);
        c2.accept(id);
        ctx("board.testnet", 0);
        c2.on_proposed(id, acc("alice.testnet"), U128(NEAR / 10), Ok(7));
        c2.on_proposal_status(id, Ok(json!({ "status": "InProgress" })));
    }));
    assert!(pending.is_err());
    // approved: the DAO paid, the point is recorded with the DAO as approver
    assert_eq!(c.on_proposal_status(id, Ok(json!({ "status": "Approved" }))), "Approved");
    assert_eq!(c.bounty(id).unwrap().bond, None, "the bond went back to the poster");
    let r = c.record_of(acc("dave.testnet"));
    assert_eq!((r.finished, r.points[0].tier.clone(), r.points[0].approved_by.clone()), (1, Tier::Dao, acc("dao.testnet")));
}

#[test]
fn a_rejected_dao_proposal_earns_nothing() {
    let mut c = board();
    let id = submitted(&mut c, NEAR);
    ctx("alice.testnet", NEAR / 10);
    c.accept(id);
    ctx("board.testnet", 0);
    c.on_proposed(id, acc("alice.testnet"), U128(NEAR / 10), Ok(3));
    c.on_proposal_status(id, Ok(json!({ "status": "Rejected" })));
    assert_eq!(c.bounty(id).unwrap().status, Status::Rejected);
    assert_eq!(c.record_of(acc("dave.testnet")).finished, 0);
}

#[test]
fn a_failed_proposal_goes_back_to_submitted() {
    let mut c = board();
    let id = submitted(&mut c, NEAR);
    ctx("alice.testnet", NEAR / 10);
    c.accept(id);
    ctx("board.testnet", 0);
    assert_eq!(c.on_proposed(id, acc("alice.testnet"), U128(NEAR / 10), Err(PromiseError::Failed)), None);
    assert_eq!(c.bounty(id).unwrap().status, Status::Submitted);
}

#[test]
#[should_panic(expected = "Not a member of the DAO council")]
fn a_non_member_cannot_approve() {
    let mut c = board();
    let id = submitted(&mut c, NEAR / 20);
    ctx("alice.testnet", 0);
    c.accept(id);
    ctx("board.testnet", 0);
    c.on_policy(id, acc("mallory.testnet"), Ok(policy(&["hq.testnet"])));
}

#[test]
#[should_panic(expected = "The contributor cannot approve their own payout")]
fn a_contributor_cannot_approve_their_own_payout() {
    let mut c = board();
    let id = submitted(&mut c, NEAR / 20);
    ctx("alice.testnet", 0);
    c.accept(id);
    ctx("dave.testnet", 0);
    let _ = c.approve(id);
}

#[test]
#[should_panic(expected = "The poster cannot claim their own bounty")]
fn a_poster_cannot_claim_their_own_bounty() {
    let mut c = board();
    ctx("alice.testnet", POST_DEPOSIT);
    let id = c.post("x".into(), U128(1));
    c.claim(id);
}

#[test]
#[should_panic(expected = "Only the poster accepts the work")]
fn only_the_poster_accepts() {
    let mut c = board();
    let id = submitted(&mut c, NEAR / 20);
    ctx("dave.testnet", 0);
    c.accept(id);
}

#[test]
#[should_panic(expected = "Only the DAO changes the tiers")]
fn only_the_dao_changes_the_tiers() {
    let mut c = board();
    ctx("hq.testnet", 0);
    c.set_tiers(tiers());
}

#[test]
#[should_panic(expected = "Petty cash too low")]
fn petty_cash_bounds_council_payouts() {
    let mut c = board();
    let id = submitted(&mut c, NEAR / 2);
    ctx("alice.testnet", 0);
    c.accept(id);
    testing_env!(VMContextBuilder::new()
        .current_account_id(acc("board.testnet"))
        .predecessor_account_id(acc("board.testnet"))
        .account_balance(NearToken::from_millinear(100))
        .build());
    c.on_policy(id, acc("hq.testnet"), Ok(policy(&["hq.testnet"])));
}

#[test]
#[should_panic(expected = "Only the DAO takes the petty cash back")]
fn only_the_dao_takes_the_petty_cash_back() {
    let mut c = board();
    ctx("hq.testnet", 0);
    let _ = c.return_petty_cash(None);
}

#[test]
fn the_dao_takes_the_petty_cash_back() {
    let mut c = board();
    ctx("dao.testnet", 0);
    let _ = c.return_petty_cash(Some(U128(NEAR)));
    assert!(get_logs().iter().any(|l| l.contains("return_petty_cash")));
}

#[test]
fn the_dao_bond_is_read_from_the_dao_and_a_short_bond_is_given_back() {
    let mut c = board();
    let id = submitted(&mut c, NEAR);
    ctx("alice.testnet", NEAR / 2);
    assert!(c.accept(id).is_some(), "get_policy promise");
    assert_eq!(c.bounty(id).unwrap().status, Status::Proposed, "cannot be accepted twice meanwhile");
    // the DAO asks 0.1 NEAR: the proposal is filed with 0.1, the 0.4 surplus goes back to the poster
    ctx("board.testnet", 0);
    let bond = json!({ "proposal_bond": (NEAR / 10).to_string() });
    assert!(c.on_bond(id, acc("alice.testnet"), U128(NEAR / 2), Ok(bond.clone())).is_some(), "add_proposal promise");
    assert_eq!(c.on_proposed(id, acc("alice.testnet"), U128(NEAR / 10), Ok(3)), Some(U64(3)));
    assert_eq!(c.bounty(id).unwrap().bond, Some(U128(NEAR / 10)), "only the DAO's bond is passed back later");
    // too little attached: back to Submitted
    let other = submitted(&mut c, NEAR);
    ctx("alice.testnet", NEAR / 100);
    c.accept(other);
    ctx("board.testnet", 0);
    assert!(c.on_bond(other, acc("alice.testnet"), U128(NEAR / 100), Ok(bond)).is_none());
    assert_eq!(c.bounty(other).unwrap().status, Status::Submitted);
}

#[test]
#[should_panic(expected = "The poster cannot approve their own bounty")]
fn a_council_member_cannot_approve_a_bounty_they_posted() {
    let mut c = board();
    ctx("hq.testnet", POST_DEPOSIT);
    let id = c.post("Draw".into(), U128(NEAR / 20));
    ctx("dave.testnet", 0);
    c.claim(id);
    c.submit(id, "ipfs://x".into());
    ctx("hq.testnet", 0);
    c.accept(id);
    ctx("board.testnet", 0);
    c.on_policy(id, acc("hq.testnet"), Ok(policy(&["hq.testnet"])));
}
