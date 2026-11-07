# Warband Raids

A daily game played by the band. Holders check in to the day's raid. If enough of the band shows up, the raiders
are minted a share, win pulls from the raid chest, and the streak grows. If the band does not show up, part of the
chest buys tokens and burns them, and the streak goes back to zero. Badges are soulbound NFTs that record what an
account did and improve its loot odds.

It replaces the yield logic with a game loop. Levels, LIFO lots and the level-weighted DAO vote stay the same.
Everything runs in the contract (`contracts/warband/src/raids.rs`), with no backend. The contract has no admin
method and every parameter is fixed at launch and readable through `config`. Upgrades are possible only while the
account keeps a full-access key: v1's is deleted, v2's and v3's are deleted on 13 Nov 2026 (README, "Keys").

## One raid

| Step | Rule |
|---|---|
| Opening | One raid per day (`day_seconds`). Day 0 is launch day, so the first raid is day 1. Its target is fixed when the raid is first touched: `target_bps` of the circulating supply. |
| Join | `join_raid()`, with no deposit, so a function-call key can check in every day. The account joins with its **seasoned balance**: lots held for at least one day. It needs at least `min_join_balance`, and can join once per raid. |
| Win | The joiners' seasoned balance reaches the target **and** at least `min_joiners` accounts joined. |
| Settlement | Done by the first action after the raid ends (a buy, sell, join, claim or `settle_raids`), before that action runs. |
| Won | The streak goes up by one. Each raider calls `claim_raid()` (or joins the next raid, which claims first) and is minted `mint_bps` of its joined balance, times the streak bonus: x1.1 at streak 1, up to x2 at streak 10. The mint is a fresh level-0 lot. The raid's `raid_pulls` loot pulls are shared among raiders in proportion to the balance they joined with. |
| Lost | `fail_burn_bps` of the chest buys tokens on the curve and burns them. The streak resets. |

**The chest.** `chest_share_bps` of every trading fee goes to the chest, before the DAO and buyback split.

**Pulls.** Each pull pays a share of the current chest:

| Tier | Odds at level 0 | Paid |
|---|---|---|
| common | 85% | 0.05% of the chest |
| rare | 12% | 0.5% |
| epic | 2.7% | 3% |
| legendary | 0.3% | 15%, plus the Legendary Pull badge |

Epic and legendary odds are multiplied by (10 + L) / 10, where L is the balance-weighted level of the account's lots
(its vote weight divided by its balance, rounded down) plus two per badge, capped at 10. The best odds are therefore
twice the base odds. Keeping a crumb of an old bag does not keep its odds: a fresh bag pulls the average down.

**Extra pulls.** `burn_for_pull()` burns tokens worth `ticket_cost_bps` of the chest. They are priced at the lower
of the spot price and yesterday's high (after days without trades, the last price carries over as yesterday's high). Even at the best odds, a pull is worth 0.353% of the chest on average. The
ticket costs 0.4%, and `new` refuses any configuration where a pull would be worth more than its ticket. The
difference is burned.

**Badges** are soulbound NEP-171 tokens (`nft_token`, `nft_tokens_for_owner`, …; `nft_transfer` always fails):

| Badge | How to earn it |
|---|---|
| First Blood | first won raid |
| Seven Days | a won raid on a streak of 7 or more |
| Legendary Pull | a legendary pull |
| Warlord | a lot held to level 10 (`claim_warlord`) |

Each badge adds two levels to the loot odds. Badges stay with the account after it sells: selling resets the
level, never the record.

## Launch parameters (picked with the simulation)

`python near/sim/raids.py` runs 60 days and five communities on the same rules as the contract. It writes
`sim/raids.png` and `sim/raids_results.md`.

| Parameter | Value | Why |
|---|---|---|
| `chest_share_bps` | 3000 (30% of the fee) | the chest stays between 1 and 3 NEAR while it pays about 10 NEAR over 60 days |
| `min_join_balance` | 1,000,000 BAND | a fake raider has to tie up a real bag |
| `min_joiners` | 10 | one whale cannot carry the raid alone (see `whale_carry`: 2 raids won out of 59) |
| `target_bps` | 2500 (25% of the supply) | an engaged band wins about 58 of 59 raids; a fading band starts losing after about four weeks |
| `mint_bps` | 10 (0.1%, up to 0.2% with the streak) | an engaged band adds about 3.5% of supply in 60 days; at 20 bps it would be +8%, at 5 bps the reward is barely visible |
| `fail_burn_bps` | 1000 | a band that stops showing up burns supply instead of printing it |
| `raid_pulls` | 20 | at most about 5% of the chest leaves per day through raid pulls |
| `ticket_cost_bps` | 40 | above the best expected pull (0.353%) |

Net supply after 60 days: **+26.5M** for an engaged band, **−4.4M** for a fading one, **−29.0M** for one
carried by a single whale. A band that plays prints a little, and a band that quits burns.

## Threat model

| Attack | What stops it | Limit, said plainly |
|---|---|---|
| Split a bag across many accounts to win more | Mints and pulls are proportional to the balance joined, so splitting earns exactly what the whole bag earns. Over 40 simulation seeds: 1.759 NEAR per 100M tokens for a bag split 12 ways, 1.780 for the same bag on one account. A transfer opens a level-0 lot, so the received tokens cannot join the same day, and the sender's odds level drops. | Splitting does help reach `min_joiners`. Faking ten raiders costs ten storage deposits (0.015 NEAR each) and ten bags of 1M seasoned tokens, held for a day after every transfer. |
| Join, then sell, and still be paid | The claim pays on min(joined balance, balance at claim). | The sold balance still counted toward the raid's success. Selling costs the 10% fee, so this does not pay. |
| Pump the price to make extra pulls cheap | Tickets are priced at min(spot, yesterday's high). Pumping today does not change yesterday's high. Tested in `a_pump_does_not_make_pulls_cheaper`. | Pumping on day N makes tickets cheaper on day N+1 only if the price is still high on day N+1, and the pump pays 10% in and 10% out. |
| Make the raid fail by changing the supply | The target is fixed when the raid opens, so buying or selling during the raid does not move it. | The joined balance is measured at join time. |
| Drain the chest with extra pulls | At the best odds a pull is worth less than its ticket (checked at launch). Each pull pays a share of the current chest, so the chest can never go negative or empty in one pull. | — |
| Choose a good random result | `env::random_seed()` mixed with the account and a counter. A caller cannot see the seed before its own transaction runs, and a pull commits in the same receipt, so there is nothing to revert. | A block producer could bias the seed. That is acceptable for testnet loot, not for large prizes. |
| A day with no activity | The first action afterwards settles up to 20 missed raids in order (`MAX_SETTLE_PER_CALL`); anyone can call `settle_raids` to move a longer stretch on. A burning lost day writes two logs and a receipt holds 100 logs and 16 KiB, so a larger batch would block every action. Empty raids leave no storage. | Every missed day burns 10% of the chest. That is a rule of the game, not a bug. |
| Storage growth | The holder deposit went from 0.0125 to 0.015 NEAR. It covers 32 lots, a pending raid and four badges (`the_storage_deposit_covers_a_full_holder`). Settled raids nobody joined are deleted. | Raid records stay (about 100 bytes each, at most one per day). |

## Real and simulated

| Part | Status |
|---|---|
| Raids, chest, pulls, badges, mints and burns | in the contract, 24 unit tests (`raid_tests.rs`), plus 19 token and math tests (43 in all) |
| Community behaviour in the simulation | invented (see the scenarios in `sim/raids.py`). The simulation shows what the rules do with it, not a forecast. |
| Loot | NEAR from the chest only. No outside NFT is deposited into the pool. |

**After the hackathon.** Pulls paid in value are a game of chance. In France, that falls under the ANJ. This is
testnet only. A mainnet launch would first need legal advice.
