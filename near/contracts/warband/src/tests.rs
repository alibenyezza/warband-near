use super::*;
use near_sdk::test_utils::VMContextBuilder;
use near_sdk::{testing_env, NearToken};

const DAY: u64 = 300; // seconds
const NS: u64 = 1_000_000_000;
const NEAR: u128 = 10u128.pow(24);
const T0: u64 = 1_700_000_000 * NS;

fn acc(s: &str) -> AccountId {
    s.parse().unwrap()
}

fn config(dao: Option<&str>) -> Config {
    Config {
        name: "Warband".into(),
        symbol: "BAND".into(),
        icon: None,
        day_seconds: DAY,
        curve_supply: U128(1_000_000_000 * ONE_TOKEN),
        virtual_near: U128(30 * NEAR),
        fee_bps: 1_000,
        dao_share_bps: if dao.is_some() { 5_000 } else { 0 },
        floor_drawdown_bps: 2_000,
        buyback_chunk_bps: 1_000,
        buyback_daily_cap_bps: 2_000,
        dao_id: dao.map(acc),
        raid: RaidConfig {
            chest_share_bps: 0,
            min_join_balance: U128(ONE_TOKEN),
            min_joiners: 1,
            target_bps: 2_500,
            mint_bps: 10,
            fail_burn_bps: 1_000,
            raid_pulls: 20,
            ticket_cost_bps: 40,
        },
    }
}

fn ctx(who: &str, deposit: u128, t: u64) {
    testing_env!(VMContextBuilder::new()
        .current_account_id(acc("warband.testnet"))
        .predecessor_account_id(acc(who))
        .attached_deposit(NearToken::from_yoctonear(deposit))
        .block_timestamp(t)
        .prepaid_gas(Gas::from_tgas(300))
        .build());
}

fn at(days: u64) -> u64 {
    T0 + days * DAY * NS + 1
}

fn setup(dao: Option<&str>) -> Contract {
    ctx("deployer.testnet", 0, T0);
    Contract::new(config(dao))
}

fn bal(c: &Contract, who: &str) -> u128 {
    c.ft_balance_of(acc(who)).0
}

#[test]
fn buy_registers_mints_and_splits_the_fee() {
    let mut c = setup(Some("band-dao.sputnik-v2.testnet"));
    ctx("alice.testnet", NEAR + STORAGE_DEPOSIT, at(0));
    let out = c.buy(None).0;
    assert!(out > 0);
    assert_eq!(bal(&c, "alice.testnet"), out);
    assert_eq!(c.ft_total_supply().0, out);
    let m = c.market();
    // 10% fee of 1 NEAR, half to the DAO, half to buybacks; 0.9 NEAR on the curve
    assert_eq!(m.dao_pending.0, NEAR / 20);
    assert_eq!(m.buyback_reserve.0, NEAR / 20);
    assert_eq!(m.real_near.0, NEAR * 9 / 10);
    assert_eq!(c.storage_balance_of(acc("alice.testnet")).unwrap().total.0, STORAGE_DEPOSIT);
}

#[test]
#[should_panic(expected = "slippage")]
fn buy_respects_min_tokens_out() {
    let mut c = setup(None);
    ctx("alice.testnet", NEAR + STORAGE_DEPOSIT, at(0));
    let q = c.quote_buy(U128(NEAR)).0;
    c.buy(Some(U128(q + 1)));
}

#[test]
fn levels_grow_one_per_day_and_cap_at_ten() {
    let mut c = setup(None);
    ctx("alice.testnet", NEAR + STORAGE_DEPOSIT, at(0));
    let out = c.buy(None).0;
    for (day, level) in [(0, 0), (1, 1), (3, 3), (10, 10), (25, 10)] {
        ctx("alice.testnet", 0, at(day));
        let a = c.account(acc("alice.testnet")).unwrap();
        assert_eq!(a.top_level, level, "day {day}");
        assert_eq!(c.weight_of(acc("alice.testnet")).0, out * level as u128);
    }
    assert_eq!(c.account(acc("alice.testnet")).unwrap().rank, "Warlord");
}

#[test]
fn transfer_in_the_middle_of_a_lot_is_lifo_and_receiver_starts_at_zero() {
    let mut c = setup(None);
    ctx("alice.testnet", NEAR + STORAGE_DEPOSIT, at(0));
    let old = c.buy(None).0;
    ctx("alice.testnet", NEAR, at(3));
    let young = c.buy(None).0;
    ctx("bob.testnet", STORAGE_DEPOSIT, at(3));
    c.storage_deposit(None, None);

    // send all of the young lot plus a quarter of the old one
    let sent = young + old / 4;
    ctx("alice.testnet", 1, at(3));
    c.ft_transfer(acc("bob.testnet"), U128(sent), None);

    let a = c.account(acc("alice.testnet")).unwrap();
    assert_eq!(a.lots.len(), 1, "the young lot is gone first");
    assert_eq!(a.lots[0].amount.0, old - old / 4);
    assert_eq!(a.lots[0].level, 3, "the old lot keeps its age");
    assert_eq!(a.weight.0, (old - old / 4) * 3);

    let b = c.account(acc("bob.testnet")).unwrap();
    assert_eq!(b.balance.0, sent);
    assert_eq!(b.top_level, 0);
    assert_eq!(b.weight.0, 0, "fresh tokens carry no vote weight");
}

#[test]
fn partial_sell_only_resets_what_is_sold() {
    let mut c = setup(None);
    ctx("alice.testnet", 2 * NEAR + STORAGE_DEPOSIT, at(0));
    let first = c.buy(None).0;
    ctx("alice.testnet", NEAR, at(5));
    let second = c.buy(None).0;
    ctx("alice.testnet", 1, at(6));
    let half = second / 2;
    let got = c.sell(U128(half), None).0;
    assert!(got > 0);
    let a = c.account(acc("alice.testnet")).unwrap();
    assert_eq!(a.lots.len(), 2);
    assert_eq!(a.lots[0].amount.0, first);
    assert_eq!(a.lots[0].level, 6);
    assert_eq!(a.lots[1].amount.0, second - half);
    assert_eq!(a.lots[1].level, 1);
    assert_eq!(c.ft_total_supply().0, first + second - half);
}

#[test]
#[should_panic(expected = "1 yoctoNEAR")]
fn sell_needs_a_full_access_key() {
    let mut c = setup(None);
    ctx("alice.testnet", NEAR + STORAGE_DEPOSIT, at(0));
    let out = c.buy(None).0;
    ctx("alice.testnet", 0, at(0));
    c.sell(U128(out), None);
}

#[test]
fn sell_round_trip_loses_only_fees() {
    let mut c = setup(None);
    ctx("alice.testnet", NEAR + STORAGE_DEPOSIT, at(0));
    let out = c.buy(None).0;
    ctx("alice.testnet", 1, at(0));
    let back = c.sell(U128(out), None).0;
    // 0.9 NEAR reached the curve, selling everything returns it minus the 10% sell fee
    let expected = NEAR * 9 / 10 * 9 / 10;
    assert!(back <= expected && expected - back < 1_000, "{back} vs {expected}");
    assert_eq!(c.market().circulating.0, 0);
}

fn dumped() -> Contract {
    let mut c = setup(None);
    ctx("whale.testnet", 20 * NEAR + STORAGE_DEPOSIT, at(0));
    let bag = c.buy(None).0;
    // the whale dumps most of the bag: the price falls more than 20% under the day's high
    ctx("whale.testnet", 1, at(0));
    c.sell(U128(bag * 3 / 4), None);
    c
}

#[test]
fn buyback_spends_a_chunk_and_burns_below_the_floor() {
    let mut c = dumped();
    let m = c.market();
    assert!(m.buyback_open, "price {} floor {}", m.price.0, m.floor_price.0);
    let reserve = m.buyback_reserve.0;
    ctx("keeper.testnet", 0, at(0));
    let burned = c.buyback().0;
    assert!(burned > 0);
    let m2 = c.market();
    assert_eq!(m2.burned.0, burned);
    assert_eq!(m2.buyback_reserve.0, reserve - reserve / 10);
    assert!(m2.price.0 > m.price.0);
    assert_eq!(c.history(Some(1))[0].kind, "buyback");
}

#[test]
#[should_panic(expected = "nothing to spend today")]
fn buyback_stops_at_the_daily_cap() {
    let mut c = dumped();
    ctx("keeper.testnet", 0, at(0));
    // cap = 20% of the reserve at the first buyback of the day: 10% of the reserve, then 10% of what is left,
    // then the remainder of the cap; the fourth call has nothing left to spend
    c.buyback();
    c.buyback();
    c.buyback();
    let m = c.market();
    assert_eq!(m.buyback_spent_today.0, m.buyback_cap_today.0);
    c.buyback();
}

#[test]
fn buyback_cap_resets_the_next_day() {
    let mut c = dumped();
    ctx("keeper.testnet", 0, at(0));
    c.buyback();
    c.buyback();
    let spent = c.market().buyback_spent_today.0;
    assert!(spent > 0);
    // next day: the reference high is still yesterday's, the price is still under the floor, a new cap opens
    ctx("keeper.testnet", 0, at(1));
    assert_eq!(c.market().buyback_spent_today.0, 0);
    assert!(c.market().buyback_open);
    c.buyback();
}

#[test]
#[should_panic(expected = "above the floor")]
fn buyback_refused_when_price_is_healthy() {
    let mut c = setup(None);
    ctx("alice.testnet", 5 * NEAR + STORAGE_DEPOSIT, at(0));
    c.buy(None);
    ctx("keeper.testnet", 0, at(0));
    c.buyback();
}

#[test]
fn lots_stay_bounded() {
    let mut c = setup(None);
    ctx("alice.testnet", NEAR / 10 + STORAGE_DEPOSIT, at(0));
    c.buy(None);
    for day in 1..60 {
        ctx("alice.testnet", NEAR / 100, at(day));
        c.buy(None);
    }
    let a = c.account(acc("alice.testnet")).unwrap();
    assert!(a.lots.len() <= MAX_LOTS);
    assert_eq!(a.balance.0, a.lots.iter().map(|l| l.amount.0).sum::<u128>());
}

#[test]
fn same_day_buys_merge_without_gaining_age() {
    let mut c = setup(None);
    ctx("alice.testnet", NEAR + STORAGE_DEPOSIT, at(0));
    c.buy(None);
    ctx("alice.testnet", NEAR, at(0) + 100 * NS);
    c.buy(None);
    let a = c.account(acc("alice.testnet")).unwrap();
    assert_eq!(a.lots.len(), 1);
    assert_eq!(a.lots[0].since.0, at(0) + 100 * NS);
}

#[test]
#[should_panic(expected = "receiver not registered")]
fn transfer_to_unregistered_fails() {
    let mut c = setup(None);
    ctx("alice.testnet", NEAR + STORAGE_DEPOSIT, at(0));
    let out = c.buy(None).0;
    ctx("alice.testnet", 1, at(0));
    c.ft_transfer(acc("nobody.testnet"), U128(out), None);
}

#[test]
fn delegated_weight_is_pulled_back_on_sell() {
    let mut c = setup(Some("band-dao.sputnik-v2.testnet"));
    ctx("alice.testnet", NEAR + STORAGE_DEPOSIT, at(0));
    let out = c.buy(None).0;
    // simulate a successful DAO registration and sync
    ctx("warband.testnet", 0, at(4));
    c.accounts.get_mut(&acc("alice.testnet")).unwrap().dao_registered = true;
    ctx("anyone.testnet", 0, at(4));
    assert!(c.sync_weight(acc("alice.testnet")).is_some());
    assert_eq!(c.account(acc("alice.testnet")).unwrap().delegated.0, out * 4);
    ctx("alice.testnet", 1, at(4));
    c.sell(U128(out / 2), None);
    assert_eq!(c.account(acc("alice.testnet")).unwrap().delegated.0, (out - out / 2) * 4);
    // nothing changed: no DAO call
    ctx("anyone.testnet", 0, at(4));
    assert!(c.sync_weight(acc("alice.testnet")).is_none());
}

#[test]
fn history_keeps_the_last_hundred() {
    let mut c = setup(None);
    ctx("alice.testnet", NEAR + STORAGE_DEPOSIT, at(0));
    c.buy(None);
    for i in 0..120 {
        ctx("alice.testnet", NEAR / 1000, at(0) + i * NS);
        c.buy(None);
    }
    let h = c.history(Some(200));
    assert_eq!(h.len(), 100);
    assert_eq!(h[0].n.0, 120);
    assert_eq!(h[99].n.0, 21);
}

#[test]
fn flush_requires_a_dao_and_a_minimum() {
    let mut c = setup(Some("band-dao.sputnik-v2.testnet"));
    ctx("alice.testnet", 5 * NEAR + STORAGE_DEPOSIT, at(0));
    c.buy(None);
    assert_eq!(c.market().dao_pending.0, NEAR / 4);
    ctx("anyone.testnet", 0, at(0));
    let _ = c.flush_to_dao();
    let m = c.market();
    assert_eq!(m.dao_pending.0, 0);
    assert_eq!(m.dao_paid.0, NEAR / 4);
}

#[test]
fn a_merge_on_an_inflow_pulls_weight_back() {
    let mut c = setup(Some("band-dao.sputnik-v2.testnet"));
    ctx("alice.testnet", NEAR / 10 + STORAGE_DEPOSIT, at(0));
    c.buy(None);
    for day in 1..MAX_LOTS as u64 {
        ctx("alice.testnet", NEAR / 100, at(day));
        c.buy(None);
    }
    assert_eq!(c.account(acc("alice.testnet")).unwrap().lots.len(), MAX_LOTS);
    ctx("warband.testnet", 0, at(40));
    c.accounts.get_mut(&acc("alice.testnet")).unwrap().dao_registered = true;
    ctx("anyone.testnet", 0, at(40));
    c.sync_weight(acc("alice.testnet"));
    // one more lot: the two youngest merge, the older one takes the newer date and loses its level
    ctx("alice.testnet", NEAR / 100, at(40));
    c.buy(None);
    let a = c.account(acc("alice.testnet")).unwrap();
    assert_eq!(a.lots.len(), MAX_LOTS);
    assert_eq!(a.delegated.0, a.weight.0, "the DAO weight follows the drop");
}

#[test]
fn a_failed_dao_call_undoes_only_its_own_change() {
    use near_sdk::test_utils::get_created_receipts;
    use near_sdk::{PromiseResult, RuntimeFeesConfig};
    let mut c = setup(Some("band-dao.sputnik-v2.testnet"));
    ctx("alice.testnet", NEAR + STORAGE_DEPOSIT, at(0));
    let out = c.buy(None).0;
    ctx("warband.testnet", 0, at(4));
    c.accounts.get_mut(&acc("alice.testnet")).unwrap().dao_registered = true;
    // push 1: 0 -> 4 x out; push 2, a day later: 4 x out -> 5 x out; both in flight, push 2 succeeds, push 1 fails
    ctx("anyone.testnet", 0, at(4));
    c.sync_weight(acc("alice.testnet"));
    ctx("anyone.testnet", 0, at(5));
    c.sync_weight(acc("alice.testnet"));
    let _ = get_created_receipts();
    assert_eq!(c.account(acc("alice.testnet")).unwrap().delegated.0, out * 5);
    testing_env!(
        VMContextBuilder::new().current_account_id(acc("warband.testnet")).predecessor_account_id(acc("warband.testnet")).block_timestamp(at(5)).build(),
        near_sdk::test_vm_config(),
        RuntimeFeesConfig::test(),
        Default::default(),
        vec![PromiseResult::Failed]
    );
    c.on_delegation(acc("alice.testnet"), U128(0), Some(U128(out * 4)));
    // the DAO holds only push 2's delta (out): the record says the same, not 0 as a plain rollback would
    assert_eq!(c.account(acc("alice.testnet")).unwrap().delegated.0, out);
}
