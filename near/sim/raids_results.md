| Scenario | Raids won / lost | Best streak | Minted | Burned (all) | of which fail burns | of which tickets | Net supply change | Chest day 60 (NEAR) | Paid by pulls (NEAR) | Legendary pulls |
|---|---|---|---|---|---|---|---|---|---|---|
| engaged | 58 / 1 | 58 | 34,023,422 | 7,534,521 | 787,028 | 0 | +26,488,901 | 1.98 | 10.24 | 8 |
| fading | 29 / 30 | 26 | 15,587,700 | 19,940,234 | 16,257,619 | 0 | -4,352,534 | 1.36 | 5.04 | 5 |
| gamblers | 58 / 1 | 58 | 35,937,035 | 17,367,345 | 787,028 | 13,400,492 | +18,569,689 | 1.98 | 9.73 | 8 |
| whale_carry | 2 / 57 | 1 | 738,449 | 29,730,075 | 22,451,497 | 0 | -28,991,626 | 1.46 | 0.10 | 0 |
| sybil_farm | 51 / 8 | 49 | 37,417,623 | 8,818,272 | 6,884,291 | 0 | +28,599,351 | 2.46 | 6.48 | 4 |

sybil_farm, day 60:
- farmer (bag split across 12 accounts on day 10): bag x1.100, 1.959 NEAR won per 100M tokens bought, 0.18 NEAR of storage paid
- twin (same size of bag, one account): bag x1.100, 1.505 NEAR won per 100M tokens bought
- raids won before the split (days 1-10): 2, after: 49
- over 40 seeds, NEAR won per 100M tokens bought: farmer 1.759, twin 1.780 (pulls are drawn by balance, so splitting wins nothing)

Parameter sweep (net supply change over 60 days, minted minus burned, in BAND):

| mint_bps | target_bps | engaged | fading | gamblers | whale_carry | sybil_farm |
|---|---|---|---|---|---|---|
| 5 | 1500 | +9,288,693 (58W) | -16,536,817 (31W) | +3,531,878 (58W) | -29,360,230 (2W) | +9,487,020 (51W) |
| 5 | 2500 | +6,905,662 (56W) | -12,036,531 (29W) | +3,531,878 (58W) | -29,360,230 (2W) | +9,487,020 (51W) |
| 5 | 4000 | +534,923 (41W) | -24,553,842 (18W) | -6,896,101 (43W) | -29,360,230 (2W) | +9,487,020 (51W) |
| 10 | 1500 | +26,488,901 (58W) | -8,624,294 (31W) | +18,569,689 (58W) | -28,991,626 (2W) | +28,599,351 (51W) |
| 10 | 2500 | +26,488,901 (58W) | -4,352,534 (29W) | +18,569,689 (58W) | -28,991,626 (2W) | +28,599,351 (51W) |
| 10 | 4000 | +13,585,111 (43W) | -19,465,768 (18W) | +4,848,428 (43W) | -28,991,626 (2W) | +28,599,351 (51W) |
| 20 | 1500 | +62,669,767 (58W) | +7,569,430 (31W) | +56,873,424 (58W) | -28,253,862 (2W) | +67,767,835 (51W) |
| 20 | 2500 | +62,669,767 (58W) | +11,359,808 (29W) | +56,873,424 (58W) | -28,253,862 (2W) | +67,767,835 (51W) |
| 20 | 4000 | +37,969,996 (43W) | -12,990,221 (18W) | +28,826,105 (43W) | -28,253,862 (2W) | +67,767,835 (51W) |

Attacks:
- Sybil head count: each fake raider costs a 0.015 NEAR storage deposit, must hold 1,000,000 seasoned tokens and waits one day after every transfer (a transfer opens a level-0 lot). Mints are proportional to balance, so splitting a bag earns exactly what the whole bag earns: the only gain is reaching min_joiners.
- Cheap tickets by pumping: tickets are priced at min(spot, yesterday's high). Pumping today raises spot but not yesterday's high, so the ticket costs the same number of tokens; the pump itself pays the 10% fee in and out.
