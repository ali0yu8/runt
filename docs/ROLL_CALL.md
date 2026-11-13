# The roll call: an exact Nakamoto coefficient, measured on chain

The Nakamoto coefficient is the fewest validators that together hold more than a third of all stake. A BFT chain needs more than two thirds of its stake online to finalize, so this is the smallest group that could stall it.

Dashboards publish this number, but a contract can't read a dashboard. To use it on chain you would normally need an oracle, and then you have to trust the oracle. NEAR doesn't need one, because the protocol gives every contract two host functions:

| Function | Returns |
|---|---|
| `env::validator_stake(account)` | That validator's stake this epoch, or 0 if it isn't one |
| `env::validator_total_stake()` | The total stake of all validators this epoch |

Together they let a contract check a full list of validators and get an exact answer.

## Why the answer is exact

The caller passes every active validator, biggest stake first, with ties in account order. The contract then requires three things:

1. **Each one is a real validator.** Its stake from the protocol must be above zero.
2. **The order is strict.** Each entry must come after the one before it, so no account can appear twice.
3. **The total matches to the last yoctoNEAR.** The stakes must add up to `validator_total_stake()`. Since nothing can be repeated, the only way to reach the total is to include everyone.

With those three, the list is the validator set, in order. The answer is where the running total first passes a third. Nobody has to be trusted, and no list can fool it: a list that leaves out a big validator fails check 3.

## The code

```rust
fn count(validators: &[AccountId]) -> u32 {
    let total = env::validator_total_stake().as_yoctonear();
    let (mut sum, mut nakamoto) = (0u128, 0u32);
    let mut previous: Option<(u128, &AccountId)> = None;
    for (i, validator) in validators.iter().enumerate() {
        let stake = env::validator_stake(validator).as_yoctonear();
        require!(stake > 0, "not an active validator");
        if let Some((last_stake, last_id)) = previous {
            require!(stake < last_stake || (stake == last_stake && validator.as_str() > last_id.as_str()), "out of order");
        }
        sum += stake;
        if nakamoto == 0 && sum * 3 > total {
            nakamoto = (i + 1) as u32;
        }
        previous = Some((stake, validator));
    }
    require!(sum == total, "incomplete list");
    nakamoto
}
```

The version in [`contract/src/lib.rs`](../contract/src/lib.rs) also returns the number of validators and the effective number of validators (one over the sum of squared shares).

## Use it from your own contract

Any contract can read the last measurement, or check a list itself:

```rust
#[ext_contract(ext_runt)]
trait Runt {
    fn nakamoto(&self) -> Option<Census>;
    fn check_roll_call(&self, validators: Vec<AccountId>) -> Roll;
}
```

A lending market could tighten its limits when the number falls. A DAO could refuse to change something sensitive while the validator set is too concentrated.

From outside a contract, the same check is a free view call:

```bash
node scripts/verify.mjs coin.runt-hq.testnet
```

## Cost

Each `validator_stake` call costs about 0.91 TGas (`validator_stake_base` in the protocol config). A real roll call of testnet's 26 validators burned 27.4 TGas, which comes to about 350 TGas for mainnet's 339. That fits in one transaction, because protocol 88 allows 1,000 TGas. A full mainnet count would burn about 0.035 NEAR.

## What it can't see

- **Who runs each validator.** The chain only knows accounts. On mainnet, pools named `cnear-1`, `cnear-2` and `cnear-3` look like one operator, and so do `figment` and `ledgerbyfigment`. Merge just the obvious same-name pools in the 9 November 2026 set and the number falls from 9 to 6. The true figure is probably lower than the one the chain can report.
- **Which moment in a round counts.** A coin keeps the first count in each round, and a long round spans several epochs, so the caller picks when it's taken.
- **Anything between epochs.** Validator stake is fixed for an epoch, so this measures the epoch you're in. Stake moved now shows up about two epochs later.
- **Whether validators are independent in practice**, such as sharing a hosting company. No on-chain number covers that.

So treat it as an upper limit on how decentralized NEAR is: exact for what it measures, and honest about what it can't.
