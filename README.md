# RUNT

**A NEAR memecoin you can only earn by keeping NEAR delegated to a small validator.** The coin never touches your NEAR. It reads your stake from the staking pool, and its supply follows how decentralized NEAR really is.

Built at NEAR HACKATHON (November 2026) for the track **Memecoins With Mechanics** (`memecoins`).

**Live page:** https://ali0yu8.github.io/runt/ reads testnet directly. Add `?coin=speedrun.runt-hq.testnet` for the coin with 5-minute rounds.

**Demo video:** [demo/runt-demo.mp4](demo/runt-demo.mp4), two minutes of the live page on testnet, where every button press is a real transaction.

| | |
|---|---|
| **What it is** | A NEP-141 token with no sale and no team share. New coins are made only when accounts prove, on chain, that their NEAR is delegated to a small validator. |
| **The problem** | On NEAR mainnet (9 November 2026), 9 of 339 validators hold more than a third of all stake. That is the smallest group that could stop NEAR finalizing. Delegators default to the biggest names, and nothing on chain pays them to choose otherwise. |
| **What is new** | The mechanic reads NEAR itself. `feed` reads a delegation straight from the staking pool, and `roll_call` measures the exact Nakamoto coefficient from the protocol's own validator numbers, with no oracle. Supply reacts to both. Other stake-based coins take custody of your NEAR and pay out its yield, often through a keeper. RUNT holds nothing, pays out no yield, needs no keeper, and cares about one thing they ignore: which validator you chose. |
| **Status** | Working on NEAR testnet. Every rule has run on the real network ([evidence below](#proven-on-testnet)). Both coins are live and have no access keys. |

## The six rules

All of them live in the contract. Every number is set once in `new` and readable through `rules()`, and the coin's account has no access keys.

1. **Stake with a runt.** Delegate NEAR to a small validator. It stays your stake, in your account, earning normal rewards.
2. **Feed it.** Once per round, `feed(pool)` reads your stake from that pool by cross-contract call. Only validators under 1% of all stake can be fed, and only pools from a locked factory.
3. **The smaller the validator, the bigger the bite.** Your points are your stake's climb up a curve, times the validator's hunger. Hunger is near 100% for the smallest validators and falls to 0 at the 1% cap. The curve is linear up to 1,000 NEAR per runt per round and a square root above that.
4. **Make the target.** Each round the pack must feed enough *different* runts. If it does, a meal is minted and shared by points, 20% goes to the Kennel, and the streak grows the next meal. The target then rises 10%.
5. **Miss it and the food spoils.** 25% of the Kennel burns, the streak resets, and the target falls 30%.
6. **Take the roll call.** Anyone can pass the full validator list to `roll_call`, once per round, for a bounty from the Kennel. The contract checks it against the protocol. If NEAR's number rises, that round's meal triples. If it falls, half the Kennel burns.

The longest streak is kept on chain and shown on the page as **Best in Show**.

## Try it: do X, see Y

### With no wallet and no account

Needs Node 20 or later.

1. From the repository's root folder, run `npm install`.
   You should see the dependencies install.
2. Run `node scripts/verify.mjs`.
   You should see `PASS` lines for NEP-141, zero access keys, the live code matching this repository's build, readable rules, balanced books, the roll call matching an independent count, four tampered roll calls being refused, and big validators being unfeedable. The last line is `Everything matches.`
3. Run `node scripts/runt.mjs litter`.
   You should see every active testnet validator, its share of stake, its hunger, and whether it can be fed.
4. Run `node scripts/runt.mjs status`.
   You should see the round, how many runts have been fed, the streak, the supply and the Kennel.
5. Open https://ali0yu8.github.io/runt/, or run `cd web && npm install && npm run dev` and open the address it prints.
   You should see the live mainnet number at the top, today's meal, the validator chart, and every mint and burn.

### See a whole round in 5 minutes

Needs a testnet account with about 3 NEAR, created with `save-to-legacy-keychain` so the scripts can sign. The speedrun coin is the same contract with 5-minute rounds.

1. Run `node scripts/runt.mjs stake kraken.pool.f863973.m0 1.2 --as you.testnet`.
   You should see `Your stake there is now 1.2 NEAR` (give or take rounding).
2. Run `node scripts/runt.mjs feed kraken.pool.f863973.m0 --as you.testnet --coin speedrun.runt-hq.testnet`.
   You should see a bite above 0. The first feed on a coin also registers you. An idle coin's target falls to 1, so one person can hit it alone.
3. Run `node scripts/runt.mjs rollcall --as you.testnet --coin speedrun.runt-hq.testnet`.
   You should see `Contract says` followed by the same number as `Counted from RPC`.
4. Run `node scripts/runt.mjs status --coin speedrun.runt-hq.testnet` until the round has ended, then `node scripts/runt.mjs claim --as you.testnet --coin speedrun.runt-hq.testnet`.
   You should see `Received` and a RUNT amount, and an `ft_mint` event in the linked transaction.
5. Do nothing for one more round, then run `settle`, with `--as` and `--coin` as before.
   You should see the streak back at 0 and `burned` higher than before, because a slice of the Kennel spoiled.

The page does the same with buttons. Add `?coin=speedrun.runt-hq.testnet` to its address.

### Run the tests and the simulation

Needs Rust and [cargo-near](https://github.com/near/cargo-near) for the tests, and Python 3 for the simulator. Rust 1.95.0 is pinned in `contract/rust-toolchain.toml`, so rustup picks it automatically. Neither needs anything else installed.

1. Run `cd contract && cargo test --lib`.
   You should see 63 tests pass.
2. Run `python sim/runt_sim.py baseline 30`.
   You should see one outcome line per scenario, the Sybil and whale checks, and supply by day.
3. Run `python sim/runt_sim.py picks`.
   You should see how each launch value compared with the alternatives across 729 parameter sets.

## Proven on testnet

These are real transactions on NEAR testnet. The rise and fall needed a chosen starting number, so they ran on two short-lived test coins that kept a key, then were deleted to recover their NEAR. Everything else ran on the live coins or the same contract.

| What was shown | Result | Transaction |
|---|---|---|
| Launch with no keys | Both coins were created, given their rules, and had their only key removed. `verify.mjs` reports `Everything matches.` on each | main [`GDcjkkai…`](https://testnet.nearblocks.io/txns/GDcjkkaiGX22LXbvienfuZgNzTuszSvCBCVqVW4PbH9U), speedrun [`2kyxpXFc…`](https://testnet.nearblocks.io/txns/2kyxpXFc55k7Gn9YCxAvAKDFCfcJvcUAA7oiL8SrSRam) |
| Proof of delegation, with no custody | `feed` read 1.1 NEAR from the staking pool and gave a bite of 0.0297, which is 93.8% hunger on the curve. No NEAR moved | [`H9Daf8YZ…`](https://testnet.nearblocks.io/txns/H9Daf8YZMrBw9NRwphmsUpD6bR3qcKFok4tj8CUeBohz) |
| The roll call on the real chain | The contract counted all 26 testnet validators, the total matched the protocol's to the last yoctoNEAR, and its answer of 2 matched an independent count from RPC | [`C8jYjuZR…`](https://testnet.nearblocks.io/txns/C8jYjuZRw3WY2sn8Mm1kkNufyzannzbNH57BqfDAe72Q) |
| A hit | Two runts fed. The 1,000,000 RUNT meal paid 800,000 to feeders, in proportion to their bites (455,839 and 344,160), and 200,000 to the Kennel | close [`E4dcW1EJ…`](https://testnet.nearblocks.io/txns/E4dcW1EJh1cNV19UM19vK8DWfvfLuCJa4B8he7J4jx9N), claims [`CHk9MG8N…`](https://testnet.nearblocks.io/txns/CHk9MG8N8e83CBQumg17LjExqKpNQXdD8ffyDFAbym4M) [`838bt7xf…`](https://testnet.nearblocks.io/txns/838bt7xfCgcbsGqgWYRfh3shE5Btdz8cdc1wBCGB1aMv) |
| The number falls | Baseline 3, roll call 2. Half the Kennel, 100,000 RUNT, burned in the same close | [`E4dcW1EJ…`](https://testnet.nearblocks.io/txns/E4dcW1EJh1cNV19UM19vK8DWfvfLuCJa4B8he7J4jx9N) |
| A miss | Nobody fed. 25% of the Kennel (25,000 RUNT) spoiled, the streak went to 0 and the target fell | [`7cwvPFsF…`](https://testnet.nearblocks.io/txns/7cwvPFsFcFWnf2Qqi9H46wUZG7RKoDFwzu7ahhL2GMs2) |
| The number rises | Baseline 1, roll call 2. The meal tripled to 3,000,000 RUNT, and the feeder received 2,400,000 | close [`4QshwweN…`](https://testnet.nearblocks.io/txns/4QshwweNKgB7tzpRjaBqjCtEJkEjSP2NnzQ4iXhGKwqo), claim [`9yBsQH9b…`](https://testnet.nearblocks.io/txns/9yBsQH9bGTezfKKGRT725pS4dZjLpkVuPN6ZWRTkaRKx) |
| The rules can't be set twice | A second `new` failed with `The contract has already been initialized` | [`9wrsoDZi…`](https://testnet.nearblocks.io/txns/9wrsoDZinACm6tjhB66qudZ68tyMuniWTE3gvuy8r5CQ) |
| Nobody can fake the pool's answer | A stranger calling `on_feed` failed with `Method on_feed is private` | [`FCHw5GgG…`](https://testnet.nearblocks.io/txns/FCHw5GgGS7fvcXRTpgC4Fmiiok15cwYCbVbu27qXPWDc) |
| Big dogs can't be fed | `feed` on a validator holding 16% failed with `RUNT[BIGDOG]` | [`5ztW4Hkm…`](https://testnet.nearblocks.io/txns/5ztW4HkmLDLDECSr8rc1feoAxFXvpfnVRuLBTQH4CyEp) |
| Only locked-factory pools count | `feed` on `node0` failed with `RUNT[POOL]` | [`AYGTbBoR…`](https://testnet.nearblocks.io/txns/AYGTbBoRtda3RAJgqfCPWXsAau9WuS8ucJ9eTp6c5NEr) |
| Only active validators count | `feed` on a pool outside the active set failed with `RUNT[ASLEEP]` | [`BsmkW5Wq…`](https://testnet.nearblocks.io/txns/BsmkW5Wq2oo32YhMcZ3ANbBS5b7FzuP53Sw4yn35ijx4) |
| A roll call can't be faked | Leaving a validator out, swapping the order and listing one twice each failed with `RUNT[ROLL]` | [`FgkHkwn2…`](https://testnet.nearblocks.io/txns/FgkHkwn21vFFMnbdZgh1ntKCWKtYHjKXNHvcbrMCfNKo) [`8aG5yt19…`](https://testnet.nearblocks.io/txns/8aG5yt19ZdfcTrxVq4JJhoXifWWrh3UyXdkqPR1rrPUN) [`FMVtmE19…`](https://testnet.nearblocks.io/txns/FMVtmE19gJ7Hqw67378r3bgC3skb6pcm1JcGb9rmZT25) |
| It is a normal NEP-141 token | A transfer without 1 yoctoNEAR failed as the standard requires | [`HaagFfda…`](https://testnet.nearblocks.io/txns/HaagFfdae3roW29aRn5PByTLd9ifyyemoE7od7N37rGa) |
| The page's buttons | Stake, feed, take the roll call, close a round and claim, all pressed on the page and signed by a local stand-in in place of a wallet app, on the live speedrun coin | stake [`9aQCsWEy…`](https://testnet.nearblocks.io/txns/9aQCsWEyKciBDpRfjVSBmPE7iRY6hg6kJ3e4FZEGSGXz) feed [`6sLmwNXt…`](https://testnet.nearblocks.io/txns/6sLmwNXtNZRvyqT4yys1sY3GCfmvxtCDEVYLK9U69NAm) roll call [`HC1nyzok…`](https://testnet.nearblocks.io/txns/HC1nyzokS8Shsiw9ocK8Uq1U4XHAymuita58zwVyTXbc) close [`FYR5KzDP…`](https://testnet.nearblocks.io/txns/FYR5KzDP7NWkkPMFcGMYrXgQcXLrSE43h7256AVh8yVq) claim [`9o16T3ML…`](https://testnet.nearblocks.io/txns/9o16T3ML5aEoDBf49Lqe4tqMW5j8wt1yGEZEzAqz7ToN) |

Live contracts, both with no access keys: the main coin [`coin.runt-hq.testnet`](https://testnet.nearblocks.io/address/coin.runt-hq.testnet) with daily rounds, and [`speedrun.runt-hq.testnet`](https://testnet.nearblocks.io/address/speedrun.runt-hq.testnet) with 5-minute rounds.

Measured on testnet: a roll call of 26 validators burned 27.4 TGas, which comes to about 350 TGas for mainnet's 339. A feed burned 8.8 TGas in total, under 0.001 NEAR.

## Against the track's requirements

| The track asks for | In RUNT | Check it |
|---|---|---|
| An NEP-141 token with the mechanic in the contract, not in a backend | One contract holds the token, rounds, Kennel and roll call. Rounds close lazily inside normal calls, so nothing needs to run off chain | `contract/src/lib.rs` |
| All parameters set at launch and readable, no admin key | Set in `new` and returned by `rules()`. There is no owner, setter or pause, and the account has no keys | `node scripts/verify.mjs` |
| Sybil resistance good enough for a demo, with limits written plainly | See [Sybil resistance and its limits](#sybil-resistance-and-its-limits) | Tests `splitting_between_accounts_on_one_runt_earns_nothing_extra` and `below_the_full_amount_points_are_proportional_so_dust_earns_dust`, and the Sybil scenario in `sim/` |
| A page with the current streak, today's progress and a log of every mint and burn | Streak and Best in Show, today's runts against the target, the latest 128 mints and burns one by one, and a table of every round's totals. All of them are also NEP-141 and NEP-297 events | The page, or `log` and `rounds` |
| Simulate 60 days under several scenarios first, showing supply over time, then pick the parameters | 10 scenarios on NEAR's real validator set, supply by day, and 729 parameter sets behind the launch values | `sim/runt_sim.py`, [results below](#the-simulation) |

## How NEAR fits

This depends on things most chains don't give a contract.

- **Validator stake as a host function.** `env::validator_stake` and `env::validator_total_stake` give any contract the protocol's own figures. That is what makes hunger and the roll call work with no oracle. [How the roll call works](docs/ROLL_CALL.md).
- **Staking pools are ordinary contracts.** A delegation can be read with one cross-contract call to `get_account_staked_balance`, so the coin proves stake without ever holding it.
- **Locked pool factories.** A pool is only as honest as its code, and a contract can't read another contract's code. But `poolv1.near` on mainnet and `pool.f863973.m0` on testnet have no access keys, and all the pools they made run one code hash, so the account name alone proves it. `verify.mjs` checks this.
- **Unbonding as Sybil cost.** Unstaked NEAR is locked for four epochs, so one balance can't hop between accounts to be counted again within a round.
- **1,000 TGas transactions.** At protocol 88 one transaction can cover a full mainnet roll call of 339 validators, about 350 TGas.
- **Keyless accounts.** The coin's account has no access keys, so "nobody can change this" is something you can check.

## What it gives NEAR

- **A reason to move stake.** On the 9 November 2026 set, moving 1,079,104 NEAR (0.19% of stake) off the biggest validator takes the number from 9 to 10. In the simulation a single 2M NEAR move does exactly that.
- **A better validator list.** The page shows stake, share, uptime and fee for every validator, with the small ones first, and lets you stake and feed in two clicks. Your NEAR stays in your own stake.
- **A decentralization figure any contract can read.** `nakamoto()` and `check_roll_call` give an exact Nakamoto coefficient with no oracle. [The guide](docs/ROLL_CALL.md) explains it and has the code.

## The simulation

`sim/runt_sim.py` starts every scenario from the real mainnet validator set in `sim/data/`, then runs the contract's rules for 60 days. The stake figures are real. The people are invented, so this is a stress test, not a forecast. Medians of 30 runs on the launch parameters:

| Scenario | Hit rate | Best streak | Supply, M | Burned, M | Number, day 60 | Top 9 share | NEAR moved | Top earner |
|---|---|---|---|---|---|---|---|---|
| Steady pack | 82% | 23 | 46.8 | 8.2 | 9 to 9 | 33.51% | 190,748 | 3.5% |
| Hype then fade | 83% | 26 | 48.1 | 8.9 | 9 to 9 | 33.52% | 27,621 | 2.5% |
| Whale moves in | 82% | 24 | 48.9 | 8.7 | 9 to 10 | 33.16% | 2,199,787 | 12.6% |
| Sybil swarm | 87% | 33 | 55.1 | 8.0 | 9 to 9 | 33.51% | 198,020 | 3.3% |
| Herd piles on one runt | 70% | 8 | 37.2 | 6.3 | 9 to 9 | 33.51% | 203,381 | 3.7% |
| Ghost town | 15% | 4 | 7.5 | 1.7 | 9 to 9 | 33.52% | 0 | 22.1% |
| Small but loyal | 77% | 11 | 40.7 | 7.4 | 9 to 9 | 33.52% | 20,437 | 11.0% |
| Two top pools merge | 82% | 23 | 46.5 | 8.5 | 9 to 8 | 36.24% | 190,748 | 3.5% |
| Whale spreads out | 83% | 28 | 51.6 | 9.0 | 9 to 10 | 33.16% | 2,194,060 | 2.9% |
| Delegators really move | 86% | 31 | 55.3 | 9.0 | 9 to 10 | 32.89% | 4,467,597 | 2.8% |

Supply over time, in millions of RUNT, median of 30 runs:

| Scenario | Day 1 | Day 5 | Day 10 | Day 20 | Day 30 | Day 40 | Day 50 | Day 60 |
|---|---|---|---|---|---|---|---|---|
| Steady pack | 0.0 | 2.0 | 7.4 | 19.7 | 30.1 | 35.9 | 41.7 | 46.8 |
| Hype then fade | 0.0 | 4.1 | 9.7 | 22.4 | 32.9 | 36.6 | 43.0 | 48.1 |
| Whale moves in | 0.0 | 2.0 | 7.4 | 22.2 | 32.5 | 38.1 | 43.9 | 48.9 |
| Sybil swarm | 0.0 | 2.0 | 7.4 | 19.7 | 34.0 | 44.4 | 49.3 | 55.1 |
| Herd piles on one runt | 0.0 | 0.0 | 3.0 | 10.3 | 18.7 | 25.2 | 31.4 | 37.2 |
| Ghost town | 0.0 | 3.0 | 5.7 | 7.6 | 7.5 | 7.5 | 7.5 | 7.5 |
| Small but loyal | 0.0 | 4.1 | 9.7 | 16.6 | 22.4 | 28.5 | 34.7 | 40.7 |
| Two top pools merge | 0.0 | 2.0 | 7.4 | 19.7 | 28.0 | 34.7 | 41.1 | 46.5 |
| Whale spreads out | 0.0 | 2.0 | 7.4 | 22.2 | 36.5 | 41.8 | 46.4 | 51.6 |
| Delegators really move | 0.0 | 2.5 | 7.8 | 22.2 | 37.0 | 44.1 | 49.4 | 55.3 |

### How the launch parameters were picked

The sweep tried 729 parameter sets, 3 runs each, on four scenarios that pull the rules in different directions. Each figure is the median of every set using that value. `python sim/runt_sim.py picks` prints this from `sim/results/sweep.json`.

| Parameter | Values tried | What changed | Picked | Why |
|---|---|---|---|---|
| Size cap | 0.5, 1, 2% of stake | A 2M NEAR whale took 3.8, 12.0, 18.1% of rewards | 1% | 169 of 339 mainnet validators qualify. A whale that moves in is still paid, without mid-sized validators drawing the rewards |
| Counts in full, per runt | 100, 1,000, 10,000 NEAR | Whale took 11.3, 12.0, 18.0%, best streak 27, 24, 14 | 1,000 NEAR | Most people's stake counts in full, and dust still earns dust |
| Target after a hit | +5, +10, +15% | Steady pack hit 90, 83, 77%, best streak 37, 24, 15 | +10% | A streak long enough to matter, and a target that catches up within a month |
| Target after a miss | −20, −30, −40% | Steady pack hit 78, 83, 85% | −30% | A miss hurts, and the pack can recover |
| Kennel's cut | 10, 20, 30% | Burned 3.6, 7.4, 11.2M of about 47M | 20% | A miss costs something real without eating the meals |
| Kennel burned on a miss | 10, 25, 50% | Burned 4.2, 7.4, 9.3M | 25% | Three missed rounds in a row cost more than half the Kennel |

The meal size only sets the scale. The streak, roll call bonus and burn weren't swept: they fire rarely, and they were chosen to be felt when they do.

The testnet coins use a stake floor of 1 NEAR and a smaller target, because testnet has only nine feedable validators and runs on faucet money. Everything else matches the values above, and `scripts/launch.mjs` shows all three presets.

## Sybil resistance and its limits

- **Stake is the ticket.** Points follow delegated NEAR, so accounts with nothing staked earn nothing.
- **Splitting between accounts on one runt earns nothing extra.** Points are steps up one curve, and the steps add up to the same climb. There is a test for this.
- **Dust earns dust.** Below 1,000 NEAR per runt per round, points are proportional to stake. In the simulation, 2,000 NEAR spread as thinly as it can go, one account on each of 169 runts, earned 844K RUNT, less than 20 accounts of 100 NEAR earned (895K).
- **Unbonding stops one balance being counted twice.** Unstaked NEAR is locked for four epochs, about 30 hours at today's block times, so moving it to another account costs more than it earns.
- **Limit: spreading across validators is rewarded, on purpose.** Above 1,000 NEAR per runt, 2,000 NEAR in one account earned 730K RUNT and the same NEAR split over 20 runts earned 895K. A whale that splits 2M NEAR over 40 runts took 50.0% of all rewards in the simulation, against 12.6% in one account. That concentrates the coin, but what it costs is exactly what NEAR wants: real stake spread over real validators.
- **Limit: the contract can't see who runs a validator.** One operator can run several pools and look like several runts. Merging only the obvious same-name pools takes mainnet's number from 9 to 6.
- **Limit: no proof of personhood.** One person can run many accounts. Nothing tries to stop that, because the cost is real stake.
- **Limit: bots can feed every round.** A person who forgets a round earns nothing that round. A script doesn't forget.

## Honest limits

- **It is a memecoin.** Nothing backs it and nothing promises it has value. It runs on testnet, and a mainnet launch would need its own legal thought.
- **Uptime isn't checked on chain.** A small validator can be unreliable, and if it is removed from the active set your stake stops earning until it returns. The page shows uptime and fee so you can choose, but the contract can't.
- **Small validators aren't automatically good for NEAR.** Too much stake on weak operators would hurt. The hunger curve is flat across hundreds of validators so you can choose reliable ones, but that depends on people looking.
- **Hunger lags.** A validator's stake is fixed for the epoch, so new stake changes its hunger about two epochs later.
- **Rounds need someone to show up.** Rounds close inside the next call, and the roll call only happens if someone takes it. Nothing is lost by waiting, but nothing happens by itself.
- **Direct delegations only.** Liquid staking tokens and lockup contracts can't feed.
- **Some real validators are left out.** The older `pool.near` factory still has full-access keys and its pools run more than one version of the code, so an account name can't prove those pools are honest. The mainnet rules accept `poolv1.near` only, which leaves 143 of 339 mainnet validators unfeedable.
- **Nothing can upgrade it.** A bug can't be patched on a keyless coin. A new coin would have to be launched instead.
- **The coins' on-chain source link is a placeholder.** They were launched before this code had a public address, so `contract_source_metadata` names a link that isn't this repository, and that can't be changed now. The code hash is the real check: `verify.mjs` compares the live code with `contract/res/runt.wasm`. `contract/Cargo.toml` keeps the same placeholder, so a rebuild matches what is deployed.
- **State grows slowly, and someone has to fund it.** A round that hits adds about 230 bytes, paid from the balance funded at launch. If that ever ran out, rounds would stop closing until someone sent the coin more NEAR, which anyone can do. The mainnet preset sets aside 5 NEAR, about six years of daily rounds.
- **Whoever takes the roll call picks the moment.** A round spans several epochs, and only the first count in a round is kept, so if the number moves at an epoch boundary, the caller decides which side of it is measured.
- **You can't leave mid-round.** An account that has fed can't unregister until that round closes. Without this, leaving and rejoining would let one account feed twice.
- **Not yet run with a wallet app.** The page's buttons were tested with a local stand-in that signs the same transactions. A real wallet's own screens haven't been tried.
- **Reviewed, not audited.** An independent AI review tried to break the contract before launch. It found a way to dilute other feeders by leaving and rejoining mid-round, and a way to burn the Kennel for free by parking tokens on it. Both are fixed, each with a test. It wasn't a professional audit.

## Repository

```
contract/         the coin: NEP-141, rounds, Kennel, roll call, and 63 unit tests
contract/res/     the built WASM that scripts/launch.mjs deploys
web/              the token page: static, reads the chain directly, no backend
scripts/          verify, launch, and everyday actions (status, litter, stake, feed, claim, settle, rollcall)
sim/              the 60-day simulator, the real validator snapshot, and its results
docs/ROLL_CALL.md how the on-chain Nakamoto measurement works, and how to use it
demo/             the demo video
```

## Libraries and AI use

Rust: `near-sdk` 5.30.1, `near-contract-standards`, `uint`, `cargo-near`. Web: Vite, `@hot-labs/near-connect`. Scripts: `near-api-js`. Fonts: Bricolage Grotesque and JetBrains Mono from Google Fonts. AI coding assistants helped write the code and docs, as the rules allow (Q-005).

## Hackathon timeline

All code here was written during the NEAR HACKATHON window, from 9 November 2026. There is no earlier baseline and no earlier product on another chain.

## License

MIT. See [LICENSE](LICENSE).
