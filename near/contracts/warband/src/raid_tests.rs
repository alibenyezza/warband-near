use super::*;
use crate::raids::TIERS;
use near_sdk::test_utils::VMContextBuilder;
use near_sdk::{testing_env, NearToken};

const DAY: u64 = 300;
const NS: u64 = 1_000_000_000;
const NEAR: u128 = 10u128.pow(24);
const T0: u64 = 1_700_000_000 * NS;

fn acc(s: &str) -> AccountId {
    s.parse().unwrap()
}

fn raid_config(min_joiners: u32) -> Config {
    Config {
        name: "Warband".into(),
        symbol: "BAND".into(),
        icon: None,
        day_seconds: DAY,
        curve_supply: U128(1_000_000_000 * ONE_TOKEN),
        virtual_near: U128(30 * NEAR),
        fee_bps: 1_000,
        dao_share_bps: 0,
        floor_drawdown_bps: 2_000,
        buyback_chunk_bps: 1_000,
        buyback_daily_cap_bps: 2_000,
        dao_id: None,
        raid: RaidConfig {
            chest_share_bps: 3_000,
            min_join_balance: U128(1_000_000 * ONE_TOKEN),
            min_joiners,
            target_bps: 2_500,
            mint_bps: 10,
            fail_burn_bps: 1_000,
            raid_pulls: 20,
            ticket_cost_bps: 40,
        },
    }
}

fn ctx(who: &str, deposit: u128, t: u64) {
    ctx_seed(who, deposit, t, [7; 32]);
}

fn ctx_seed(who: &str, deposit: u128, t: u64, seed: [u8; 32]) {
    testing_env!(VMContextBuilder::new()
        .current_account_id(acc("warband.testnet"))
        .predecessor_account_id(acc(who))
        .attached_deposit(NearToken::from_yoctonear(deposit))
        .block_timestamp(t)
        .random_seed(seed)
        .prepaid_gas(Gas::from_tgas(300))
        .build());
}

fn at(days: u64) -> u64 {
    T0 + days * DAY * NS + 1
}

fn setup(min_joiners: u32) -> Contract {
    ctx("deployer.testnet", 0, T0);
    Contract::new(raid_config(min_joiners))
}

fn buy(c: &mut Contract, who: &str, near: u128, day: u64) -> u128 {
    ctx(who, near + STORAGE_DEPOSIT, at(day));
    c.buy(None).0
}

fn bal(c: &Contract, who: &str) -> u128 {
    c.ft_balance_of(acc(who)).0
}

/// Alice and Bob buy on day 0 and both join on day 1.
fn two_raiders(min_joiners: u32) -> (Contract, u128, u128) {
    let mut c = setup(min_joiners);
    let a = buy(&mut c, "alice.testnet", 5 * NEAR, 0);
    let b = buy(&mut c, "bob.testnet", 3 * NEAR, 0);
    ctx("alice.testnet", 0, at(1));
    assert_eq!(c.join_raid().0, a);
    ctx("bob.testnet", 0, at(1));
    assert_eq!(c.join_raid().0, b);
    (c, a, b)
}

#[test]
fn a_share_of_every_fee_fills_the_chest() {
    let mut c = setup(1);
    buy(&mut c, "alice.testnet", NEAR, 0);
    // 10% fee of 1 NEAR, 30% of it to the chest, the rest to buybacks (no DAO here)
    assert_eq!(c.raid_status().chest.0, NEAR * 3 / 100);
    assert_eq!(c.market().buyback_reserve.0, NEAR * 7 / 100);
}

#[test]
#[should_panic(expected = "not enough seasoned tokens")]
fn fresh_tokens_cannot_join() {
    let mut c = setup(1);
    buy(&mut c, "alice.testnet", NEAR, 0);
    ctx("alice.testnet", 0, at(0));
    c.join_raid();
}

#[test]
#[should_panic(expected = "already in today's raid")]
fn one_join_per_raid() {
    let (mut c, _, _) = two_raiders(2);
    ctx("alice.testnet", 0, at(1));
    c.join_raid();
}

#[test]
#[should_panic(expected = "not enough seasoned tokens")]
fn a_split_bag_cannot_join_twice() {
    let mut c = setup(1);
    let a = buy(&mut c, "alice.testnet", 5 * NEAR, 0);
    ctx("bob.testnet", STORAGE_DEPOSIT, at(1));
    c.storage_deposit(None, None);
    ctx("alice.testnet", 0, at(1));
    c.join_raid();
    ctx("alice.testnet", 1, at(1));
    c.ft_transfer(acc("bob.testnet"), U128(a / 2), None);
    // the tokens Bob received open a level-0 lot: they do not count today
    ctx("bob.testnet", 0, at(1));
    c.join_raid();
}

#[test]
fn a_won_raid_mints_grows_the_streak_and_pays_loot() {
    let (mut c, a, b) = two_raiders(2);
    let chest_before = c.raid_status().chest.0;
    assert!(chest_before > 0);
    ctx("alice.testnet", 0, at(2));
    let claim = c.claim_raid();
    assert!(claim.won);
    // 0.1% of the joined balance, x1.1 for a streak of one
    assert_eq!(claim.minted.0, a / 1_000 * 11 / 10);
    assert_eq!(bal(&c, "alice.testnet"), a + claim.minted.0);
    // Alice holds 5/8 of the joined balance: 12 or 13 of the 20 pulls
    let expected = 20 * a / (a + b);
    assert!(claim.pulls.len() as u128 == expected || claim.pulls.len() as u128 == expected + 1, "{}", claim.pulls.len());
    let won: u128 = claim.pulls.iter().map(|p| p.near.0).sum();
    assert!(won > 0);
    assert_eq!(c.raid_status().chest.0, chest_before - won);
    assert!(claim.badges.contains(&"First Blood".to_string()));
    let s = c.raid_status();
    assert_eq!((s.streak, s.won, s.lost), (1, 1, 0));
    // the minted tokens are a fresh lot: they earn no seniority
    let acct = c.account(acc("alice.testnet")).unwrap();
    assert_eq!(acct.lots.last().unwrap().level, 0);
    assert_eq!(c.nft_supply_for_owner(acc("alice.testnet")).0, 1);
}

#[test]
fn a_lost_raid_burns_part_of_the_chest_and_resets_the_streak() {
    // two joiners needed, only Alice shows up
    let mut c = setup(2);
    buy(&mut c, "alice.testnet", 5 * NEAR, 0);
    ctx("alice.testnet", 0, at(1));
    c.join_raid();
    let chest = c.raid_status().chest.0;
    let burned = c.market().burned.0;
    ctx("alice.testnet", 0, at(2));
    let claim = c.claim_raid();
    assert!(!claim.won);
    assert_eq!(claim.minted.0, 0);
    assert!(claim.pulls.is_empty());
    let s = c.raid_status();
    assert_eq!((s.streak, s.won, s.lost), (0, 0, 1));
    assert!(s.chest.0 < chest);
    assert!(c.market().burned.0 > burned);
    assert!(c.raid(U64(1)).unwrap().burned.0 > 0);
}

#[test]
fn tokens_sold_after_joining_are_not_paid_for() {
    let (mut c, a, _) = two_raiders(2);
    ctx("alice.testnet", 1, at(1));
    c.sell(U128(a / 2), None);
    ctx("alice.testnet", 0, at(2));
    let claim = c.claim_raid();
    assert!(claim.won, "the raid counted the balance joined");
    assert_eq!(claim.minted.0, (a - a / 2) / 1_000 * 11 / 10);
}

#[test]
fn joining_again_pays_the_last_raid_first() {
    let (mut c, a, _) = two_raiders(2);
    ctx("bob.testnet", 0, at(2));
    c.join_raid();
    ctx("alice.testnet", 0, at(2));
    let joined = c.join_raid().0;
    // Alice was paid for day 1 on the way in; the mint is fresh so only `a` is seasoned today
    assert_eq!(joined, a);
    assert!(bal(&c, "alice.testnet") > a);
    assert_eq!(c.raid_of(acc("alice.testnet")).unwrap().raid.0, 2);
    ctx("alice.testnet", 0, at(3));
    let claim = c.claim_raid();
    assert!(claim.won);
    assert_eq!(c.raid_status().streak, 2);
    assert_eq!(claim.minted.0, a / 1_000 * 12 / 10, "x1.2 at streak two");
}

#[test]
#[should_panic(expected = "the raid is not over yet")]
fn no_claim_before_the_raid_ends() {
    let (mut c, _, _) = two_raiders(2);
    ctx("alice.testnet", 0, at(1));
    c.claim_raid();
}

#[test]
fn empty_days_leave_no_raid_and_burn_the_chest_slice() {
    let mut c = setup(1);
    buy(&mut c, "alice.testnet", 10 * NEAR, 0);
    let chest = c.raid_status().chest.0;
    ctx("anyone.testnet", 0, at(5));
    assert_eq!(c.settle_raids().0, 5);
    let s = c.raid_status();
    // day 0 is launch day, not a raid; days 1 to 4 were lost
    assert_eq!(s.lost, 4);
    // 10% of the chest burned each lost day: 0.9^4
    let left = (0..4).fold(chest, |x, _| x - x / 10);
    assert!(s.chest.0.abs_diff(left) <= 5, "{} vs {left}", s.chest.0);
    // each burning day stays in the ledger with what it burned
    assert!(c.raid(U64(2)).unwrap().burned.0 > 0);
}

#[test]
fn a_pump_does_not_make_pulls_cheaper() {
    let (mut c, _, _) = two_raiders(2);
    ctx("alice.testnet", 0, at(1));
    let before = c.raid_status().ticket_tokens.0;
    let chest = c.raid_status().chest.0;
    let price = c.market().price.0;
    // a whale pumps the spot price today
    buy(&mut c, "whale.testnet", 20 * NEAR, 1);
    assert!(c.market().price.0 > price * 2);
    let s = c.raid_status();
    // the chest grew with the fee, the price used did not move: the cost grows with the chest only
    let expected = crate::math::mul_div(before, s.chest.0, chest);
    assert!(s.ticket_tokens.0.abs_diff(expected) <= expected / 1_000_000, "{} vs {expected}", s.ticket_tokens.0);
}

#[test]
fn burning_for_a_pull_costs_tokens_and_pays_from_the_chest() {
    let (mut c, a, _) = two_raiders(2);
    ctx("alice.testnet", 1, at(1));
    let cost = c.raid_status().ticket_tokens.0;
    let chest = c.raid_status().chest.0;
    let supply = c.ft_total_supply().0;
    let pull = c.burn_for_pull();
    assert_eq!(bal(&c, "alice.testnet"), a - cost);
    assert_eq!(c.ft_total_supply().0, supply - cost);
    assert_eq!(c.raid_status().chest.0, chest - pull.near.0);
    assert_eq!(c.history(Some(1))[0].kind, "pull");
}

#[test]
#[should_panic(expected = "1 yoctoNEAR")]
fn burning_for_a_pull_needs_a_full_access_key() {
    let (mut c, _, _) = two_raiders(2);
    ctx("alice.testnet", 0, at(1));
    c.burn_for_pull();
}

#[test]
fn loot_tiers_match_the_odds_over_many_pulls() {
    let mut c = setup(1);
    buy(&mut c, "alice.testnet", 20 * NEAR, 0);
    let mut seen = [0u32; 4];
    for i in 0..2_000u32 {
        // refill the chest so it never runs dry; the tier does not depend on its size
        c.chest = NEAR;
        let mut seed = [0u8; 32];
        seed[..4].copy_from_slice(&i.to_le_bytes());
        ctx_seed("alice.testnet", 0, at(0), seed);
        let mut badges = vec![];
        let p = c.pull(&acc("alice.testnet"), &mut badges);
        seen[TIERS.iter().position(|t| t.0 == p.tier).unwrap()] += 1;
    }
    // level 0, no badge: 85% / 12% / 2.7% / 0.3%
    assert!((1_620..=1_780).contains(&seen[0]), "{seen:?}");
    assert!((190..=290).contains(&seen[1]), "{seen:?}");
    assert!(seen[2] + seen[3] >= 25 && seen[2] + seen[3] <= 95, "{seen:?}");
}

#[test]
fn level_and_badges_raise_the_odds_up_to_double() {
    let mut c = setup(1);
    buy(&mut c, "alice.testnet", NEAR, 0);
    assert_eq!(c.pull_odds(acc("alice.testnet")), vec![8_500, 1_200, 270, 30]);
    ctx("alice.testnet", 0, at(4));
    assert_eq!(c.pull_odds(acc("alice.testnet")), vec![8_380, 1_200, 378, 42]);
    ctx("alice.testnet", 0, at(30));
    assert_eq!(c.pull_odds(acc("alice.testnet")), vec![8_200, 1_200, 540, 60]);
}

#[test]
fn warlord_badge_after_ten_days_once() {
    let mut c = setup(1);
    buy(&mut c, "alice.testnet", NEAR, 0);
    ctx("alice.testnet", 0, at(10));
    assert_eq!(c.claim_warlord(), "Warlord");
    let t = c.nft_tokens_for_owner(acc("alice.testnet"), None, None);
    assert_eq!(t.len(), 1);
    assert_eq!(t[0].metadata.title.as_deref(), Some("Warlord"));
    assert_eq!(c.nft_token("0".into()).unwrap().owner_id, acc("alice.testnet"));
    assert_eq!(c.nft_total_supply().0, 1);
}

#[test]
#[should_panic(expected = "already a Warlord")]
fn warlord_badge_is_minted_once() {
    let mut c = setup(1);
    buy(&mut c, "alice.testnet", NEAR, 0);
    ctx("alice.testnet", 0, at(10));
    c.claim_warlord();
    c.claim_warlord();
}

#[test]
#[should_panic(expected = "soulbound")]
fn badges_cannot_be_transferred() {
    let mut c = setup(1);
    buy(&mut c, "alice.testnet", NEAR, 0);
    ctx("alice.testnet", 0, at(10));
    c.claim_warlord();
    ctx("alice.testnet", 1, at(10));
    c.nft_transfer(acc("bob.testnet"), "0".into(), None, None);
}

#[test]
#[should_panic(expected = "a pull is worth more than its ticket")]
fn launch_refuses_a_ticket_cheaper_than_the_best_pull() {
    let mut cfg = raid_config(1);
    // the best odds pay 0.353% of the chest per pull on average
    cfg.raid.ticket_cost_bps = 30;
    ctx("deployer.testnet", 0, T0);
    Contract::new(cfg);
}

#[test]
fn the_storage_deposit_covers_a_full_holder() {
    let mut c = setup(1);
    buy(&mut c, "alice.testnet", NEAR, 0);
    let before = env::storage_usage();
    // worst case: 32 lots, a pending raid and four badges
    let h = c.accounts.get_mut(&acc("alice.testnet")).unwrap();
    h.lots = (0..MAX_LOTS as u64).map(|i| Lot { amount: U128(u128::MAX), since: U64(i) }).collect();
    h.joined = Some(Joined { raid: U64(u64::MAX), amount: U128(u128::MAX) });
    for kind in 0..4 {
        let i = c.badges.len();
        c.badges.push(Badge { owner_id: acc("alice.testnet"), kind, issued_at: U64(0) });
        c.accounts.get_mut(&acc("alice.testnet")).unwrap().badges.push(i);
    }
    c.accounts.flush();
    c.badges.flush();
    let used = (env::storage_usage() - before) as u128 + 200; // + the empty holder entry
    assert!(used * env::storage_byte_cost().as_yoctonear() < STORAGE_DEPOSIT, "{used} bytes");
}
