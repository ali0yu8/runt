//! Unit tests. The validator set is mocked, which is the only way to make NEAR's real numbers move on demand.

use super::*;
use near_sdk::test_utils::{VMContextBuilder, get_logs};
use near_sdk::{PromiseResult, RuntimeFeesConfig, test_vm_config, testing_env};
use std::collections::HashMap;

const DAY: u64 = 86_400 * NANOS_PER_SECOND;
const START: u64 = 1_000 * NANOS_PER_SECOND;
const TOKEN: u128 = 1_000_000_000_000_000_000;

fn id(name: &str) -> AccountId {
    name.parse().unwrap()
}

fn near(n: u128) -> NearToken {
    NearToken::from_yoctonear(n * ONE_NEAR)
}

fn rules() -> Rules {
    Rules {
        round_seconds: 86_400,
        meal: U128(1_000_000 * TOKEN),
        streak_step_bps: 200,
        streak_cap: 30,
        kennel_bps: 2_000,
        spoil_bps: 2_500,
        target_start: 2,
        target_floor: 1,
        target_up_bps: 1_000,
        target_down_bps: 3_000,
        size_cap_bps: 100,
        min_stake_near: 10,
        full_bite_near: 1_000,
        census_bounty: U128(5_000 * TOKEN),
        nakamoto_up_bonus_bps: 20_000,
        nakamoto_down_burn_bps: 5_000,
        baseline_nakamoto: 0,
        pool_suffixes: vec!["poolv1.near".to_string(), "pool.near".to_string()],
    }
}

/// A small stand-in for the chain: who is calling, when, and the validator set.
struct Chain {
    now: u64,
    validators: HashMap<String, NearToken>,
}

impl Chain {
    fn new() -> Self {
        let mut chain = Self { now: START, validators: HashMap::new() };
        chain.set(&[
            ("biggest.poolv1.near", 400_000),
            ("big2.poolv1.near", 300_000),
            ("big3.poolv1.near", 150_000),
            ("runt-a.poolv1.near", 1_000),
            ("runt-b.pool.near", 2_000),
            ("runt-c.poolv1.near", 5_000),
            ("solo-validator.near", 142_000),
        ]);
        chain
    }

    fn set(&mut self, list: &[(&str, u128)]) {
        self.validators = list.iter().map(|(name, stake)| (name.to_string(), near(*stake))).collect();
    }

    fn call(&self, who: &str, deposit: NearToken) {
        self.call_with_results(who, deposit, vec![]);
    }

    fn call_with_results(&self, who: &str, deposit: NearToken, results: Vec<PromiseResult>) {
        let context = VMContextBuilder::new()
            .current_account_id(id("runt.near"))
            .signer_account_id(id(who))
            .predecessor_account_id(id(who))
            .block_timestamp(self.now)
            .attached_deposit(deposit)
            .prepaid_gas(Gas::from_tgas(300))
            .storage_usage(env::storage_usage())
            .epoch_height(100)
            .build();
        testing_env!(context, test_vm_config(), RuntimeFeesConfig::test(), self.validators.clone(), results);
    }

    fn as_user(&self, who: &str) {
        self.call(who, NearToken::from_yoctonear(0));
    }

    fn as_contract(&self) {
        self.as_user("runt.near");
    }

    fn next_round(&mut self) {
        self.now += DAY;
    }

    /// The list a correct caller would send: biggest first, ties in account order.
    fn sorted(&self) -> Vec<AccountId> {
        let mut list: Vec<(AccountId, u128)> = self.validators.iter().map(|(k, v)| (id(k), v.as_yoctonear())).collect();
        list.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.as_str().cmp(b.0.as_str())));
        list.into_iter().map(|x| x.0).collect()
    }
}

fn launch(chain: &Chain, rules: Rules) -> Runt {
    chain.as_contract();
    Runt::new(rules)
}

fn register(chain: &Chain, coin: &mut Runt, who: &str) {
    chain.call(who, NearToken::from_millinear(10));
    coin.storage_deposit(None, Some(true));
}

/// Runs a whole feed: the call, then the pool's answer.
fn feed(chain: &Chain, coin: &mut Runt, who: &str, pool: &str, stake_near: u128) -> u128 {
    chain.as_user(who);
    let _ = coin.feed(id(pool));
    let (round, hunger) = {
        let plate = coin.plate_of(id(who)).expect("plate should be marked before the pool call");
        assert!(plate.pending);
        (plate.round, coin.hunger_of(id(pool)).hunger_bps)
    };
    chain.as_contract();
    coin.on_feed(id(who), id(pool), round, hunger, Ok(U128(stake_near * ONE_NEAR))).0
}

fn balance(chain: &Chain, coin: &Runt, who: &str) -> u128 {
    chain.as_user(who);
    coin.ft_balance_of(id(who)).0
}

fn kennel(chain: &Chain, coin: &Runt) -> u128 {
    chain.as_contract();
    coin.status().kennel.0
}

fn assert_books_balance(chain: &Chain, coin: &Runt) {
    chain.as_contract();
    let status = coin.status();
    assert_eq!(status.total_supply.0, status.minted.0 - status.burned.0, "supply must be everything minted minus everything burned");
}

// ---- Parameters match the simulator -----------------------------------------------------------

#[test]
fn hunger_and_bites_match_the_simulator() {
    let data: Value = near_sdk::serde_json::from_str(include_str!("../../sim/vectors.json")).unwrap();
    let cap = data["size_cap_bps"].as_u64().unwrap() as u32;
    for case in data["hunger"].as_array().unwrap() {
        let share = u128::from(case["share_bps"].as_u64().unwrap());
        let total = 10_000_000 * ONE_NEAR;
        let stake = total * share / 10_000;
        let expected = case["hunger_bps"].as_u64().unwrap() as u32;
        assert_eq!(hunger_bps(stake, total, cap), expected, "hunger at {share} bps");
    }
    let full = data["full_bite"].as_u64().unwrap() as u128;
    for case in data["bites"].as_array().unwrap() {
        let got = bite_micro(
            case["hunger_bps"].as_u64().unwrap() as u32,
            u128::from(case["before"].as_u64().unwrap()),
            u128::from(case["stake"].as_u64().unwrap()),
            full,
        ) as i128;
        let expected = case["bite_micro"].as_i64().unwrap() as i128;
        assert!((got - expected).abs() <= 3, "bite {case}: got {got}, simulator says {expected}");
    }
}

// ---- Hunger and bites -------------------------------------------------------------------------

#[test]
fn smaller_validators_are_hungrier() {
    let total = 1_000_000 * ONE_NEAR;
    let hunger = |stake: u128| hunger_bps(stake * ONE_NEAR, total, 100);
    assert!(hunger(100) > hunger(1_000));
    assert!(hunger(1_000) > hunger(5_000));
    assert_eq!(hunger(10_000), 0, "exactly at the cap");
    assert_eq!(hunger(20_000), 0, "above the cap");
    assert_eq!(hunger_bps(0, 0, 100), 0, "no validators at all");
}

#[test]
fn splitting_between_accounts_on_one_runt_earns_nothing_extra() {
    let full = 1_000;
    let whole = bite_micro(10_000, 0, 5_000, full);
    let mut parts = 0;
    let mut fed = 0;
    for _ in 0..50 {
        parts += bite_micro(10_000, fed, 100, full);
        fed += 100;
    }
    let difference = (whole as i128 - parts as i128).abs();
    assert!(difference <= 60, "50 accounts earned {parts}, one account {whole}");
}

#[test]
fn below_the_full_amount_points_are_proportional_so_dust_earns_dust() {
    let one = bite_micro(10_000, 0, 10, 1_000);
    let hundred = bite_micro(10_000, 0, 1_000, 1_000);
    let ratio = hundred as f64 / one as f64;
    assert!((ratio - 100.0).abs() < 0.01, "ratio {ratio}");
}

#[test]
fn above_the_full_amount_spreading_beats_piling_on() {
    let one_runt = bite_micro(10_000, 0, 40_000, 1_000);
    let ten_runts = 10 * bite_micro(10_000, 0, 4_000, 1_000);
    assert!(ten_runts > 3 * one_runt, "ten runts {ten_runts}, one runt {one_runt}");
}

#[test]
fn the_curve_is_continuous_at_the_full_amount() {
    let just_below = curve_micro(1_000, 1_000);
    let just_above = curve_micro(1_001, 1_000);
    assert!(just_above >= just_below && just_above - just_below < 20_000);
}

#[test]
fn integer_square_root_is_exact() {
    for n in [0u128, 1, 2, 3, 4, 15, 16, 17, 1_000_000, 999_999_999_999, u128::from(u64::MAX)] {
        let root = isqrt(n);
        assert!(root * root <= n && (root + 1) * (root + 1) > n, "isqrt({n}) = {root}");
    }
}

// ---- Who can feed -----------------------------------------------------------------------------

#[test]
#[should_panic(expected = "RUNT[REGISTER]")]
fn must_register_first() {
    let chain = Chain::new();
    let mut coin = launch(&chain, rules());
    chain.as_user("alice.near");
    let _ = coin.feed(id("runt-a.poolv1.near"));
}

#[test]
#[should_panic(expected = "RUNT[POOL]")]
fn a_validator_that_is_not_a_factory_pool_is_refused() {
    let chain = Chain::new();
    let mut coin = launch(&chain, rules());
    register(&chain, &mut coin, "alice.near");
    chain.as_user("alice.near");
    let _ = coin.feed(id("solo-validator.near"));
}

#[test]
#[should_panic(expected = "RUNT[POOL]")]
fn a_lookalike_suffix_is_refused() {
    let chain = Chain::new();
    let mut coin = launch(&chain, rules());
    register(&chain, &mut coin, "alice.near");
    chain.as_user("alice.near");
    let _ = coin.feed(id("evil-poolv1.near"));
}

#[test]
#[should_panic(expected = "RUNT[ASLEEP]")]
fn a_pool_outside_the_active_set_is_refused() {
    let chain = Chain::new();
    let mut coin = launch(&chain, rules());
    register(&chain, &mut coin, "alice.near");
    chain.as_user("alice.near");
    let _ = coin.feed(id("sleeping.poolv1.near"));
}

#[test]
#[should_panic(expected = "RUNT[BIGDOG]")]
fn a_big_dog_cannot_be_fed() {
    let chain = Chain::new();
    let mut coin = launch(&chain, rules());
    register(&chain, &mut coin, "alice.near");
    chain.as_user("alice.near");
    let _ = coin.feed(id("biggest.poolv1.near"));
}

#[test]
#[should_panic(expected = "RUNT[FED]")]
fn one_feed_per_account_per_round() {
    let chain = Chain::new();
    let mut coin = launch(&chain, rules());
    register(&chain, &mut coin, "alice.near");
    feed(&chain, &mut coin, "alice.near", "runt-a.poolv1.near", 100);
    chain.as_user("alice.near");
    let _ = coin.feed(id("runt-b.pool.near"));
}

#[test]
#[should_panic(expected = "RUNT[FED]")]
fn a_second_feed_cannot_slip_in_while_the_pool_answers() {
    let chain = Chain::new();
    let mut coin = launch(&chain, rules());
    register(&chain, &mut coin, "alice.near");
    chain.as_user("alice.near");
    let _ = coin.feed(id("runt-a.poolv1.near"));
    let _ = coin.feed(id("runt-b.pool.near"));
}

#[test]
fn too_little_stake_clears_the_plate_and_can_retry() {
    let chain = Chain::new();
    let mut coin = launch(&chain, rules());
    register(&chain, &mut coin, "alice.near");
    assert_eq!(feed(&chain, &mut coin, "alice.near", "runt-a.poolv1.near", 9), 0);
    chain.as_user("alice.near");
    assert!(coin.plate_of(id("alice.near")).is_none());
    assert_eq!(coin.status().open.plates, 0);
    assert!(feed(&chain, &mut coin, "alice.near", "runt-a.poolv1.near", 10) > 0);
}

#[test]
fn a_failed_pool_read_clears_the_plate() {
    let chain = Chain::new();
    let mut coin = launch(&chain, rules());
    register(&chain, &mut coin, "alice.near");
    chain.as_user("alice.near");
    let _ = coin.feed(id("runt-a.poolv1.near"));
    chain.as_contract();
    coin.on_feed(id("alice.near"), id("runt-a.poolv1.near"), 0, 9_000, Err(PromiseError::Failed));
    assert!(coin.plate_of(id("alice.near")).is_none());
}

#[test]
fn an_answer_arriving_after_the_round_closed_is_dropped() {
    let mut chain = Chain::new();
    let mut coin = launch(&chain, rules());
    register(&chain, &mut coin, "alice.near");
    chain.as_user("alice.near");
    let _ = coin.feed(id("runt-a.poolv1.near"));
    chain.next_round();
    chain.as_user("bob.near");
    coin.settle();
    chain.as_contract();
    assert_eq!(coin.on_feed(id("alice.near"), id("runt-a.poolv1.near"), 0, 9_000, Ok(U128(100 * ONE_NEAR))).0, 0);
    assert_eq!(coin.status().open.feeders, 0);
}

// ---- Rounds -----------------------------------------------------------------------------------

#[test]
fn a_hit_mints_a_meal_shared_by_bites() {
    let mut chain = Chain::new();
    let mut coin = launch(&chain, rules());
    for who in ["alice.near", "bob.near"] {
        register(&chain, &mut coin, who);
    }
    let alice = feed(&chain, &mut coin, "alice.near", "runt-a.poolv1.near", 400);
    let bob = feed(&chain, &mut coin, "bob.near", "runt-b.pool.near", 100);
    chain.next_round();
    chain.as_user("alice.near");
    let paid_alice = coin.claim().0;
    chain.as_user("bob.near");
    let paid_bob = coin.claim().0;

    let pot = 800_000 * TOKEN;
    assert_eq!(paid_alice, mul_div(pot, alice, alice + bob));
    assert_eq!(paid_bob, mul_div(pot, bob, alice + bob));
    assert_eq!(kennel(&chain, &coin), 200_000 * TOKEN, "the Kennel takes 20% of the meal");
    chain.as_contract();
    let status = coin.status();
    assert_eq!(status.streak, 1);
    assert_eq!(status.target, 3, "target 2 up 10% rounds up to 3");
    assert_books_balance(&chain, &coin);
}

#[test]
fn no_early_claim() {
    let chain = Chain::new();
    let mut coin = launch(&chain, rules());
    register(&chain, &mut coin, "alice.near");
    feed(&chain, &mut coin, "alice.near", "runt-a.poolv1.near", 100);
    chain.as_user("alice.near");
    assert_eq!(coin.claim().0, 0);
    assert_eq!(balance(&chain, &coin, "alice.near"), 0);
}

#[test]
fn no_double_claim() {
    let mut chain = Chain::new();
    let mut coin = launch(&chain, Rules { target_start: 1, ..rules() });
    register(&chain, &mut coin, "alice.near");
    feed(&chain, &mut coin, "alice.near", "runt-a.poolv1.near", 100);
    chain.next_round();
    chain.as_user("alice.near");
    assert!(coin.claim().0 > 0);
    assert_eq!(coin.claim().0, 0);
}

#[test]
fn feeding_again_collects_the_last_meal() {
    let mut chain = Chain::new();
    let mut coin = launch(&chain, Rules { target_start: 1, ..rules() });
    register(&chain, &mut coin, "alice.near");
    feed(&chain, &mut coin, "alice.near", "runt-a.poolv1.near", 100);
    chain.next_round();
    feed(&chain, &mut coin, "alice.near", "runt-b.pool.near", 100);
    assert_eq!(balance(&chain, &coin, "alice.near"), 800_000 * TOKEN);
}

#[test]
fn a_view_can_tell_you_what_is_claimable() {
    let mut chain = Chain::new();
    let mut coin = launch(&chain, Rules { target_start: 1, ..rules() });
    register(&chain, &mut coin, "alice.near");
    feed(&chain, &mut coin, "alice.near", "runt-a.poolv1.near", 100);
    chain.as_user("alice.near");
    assert_eq!(coin.claimable(id("alice.near")).0, 0, "round still open");
    chain.next_round();
    chain.as_user("alice.near");
    assert_eq!(coin.claimable(id("alice.near")).0, 800_000 * TOKEN, "time is up but nobody has closed it");
    coin.settle();
    assert_eq!(coin.claimable(id("alice.near")).0, 800_000 * TOKEN, "closed");
}

#[test]
fn one_runt_counts_once_however_many_feed_it() {
    let mut chain = Chain::new();
    let mut coin = launch(&chain, rules());
    for who in ["a.near", "b.near", "c.near"] {
        register(&chain, &mut coin, who);
        feed(&chain, &mut coin, who, "runt-a.poolv1.near", 100);
    }
    chain.as_contract();
    assert_eq!(coin.status().open.plates, 1);
    chain.next_round();
    chain.as_user("a.near");
    coin.settle();
    let status = coin.status();
    assert_eq!(status.streak, 0, "three feeders on one runt can't hit a target of two runts");
    assert_eq!(status.minted.0, 0);
}

#[test]
fn a_miss_burns_part_of_the_kennel_and_resets_the_streak() {
    let mut chain = Chain::new();
    let mut coin = launch(&chain, Rules { target_start: 1, ..rules() });
    register(&chain, &mut coin, "alice.near");
    feed(&chain, &mut coin, "alice.near", "runt-a.poolv1.near", 100);
    chain.next_round();
    chain.as_user("alice.near");
    coin.settle();
    let before = kennel(&chain, &coin);
    assert_eq!(coin.status().streak, 1);
    chain.next_round();
    chain.as_user("alice.near");
    coin.settle();
    let status = coin.status();
    assert_eq!(status.streak, 0);
    assert_eq!(kennel(&chain, &coin), before - before / 4);
    assert_eq!(status.burned.0, before / 4);
    assert_books_balance(&chain, &coin);
}

#[test]
fn the_target_ratchets_and_respects_its_floor() {
    let mut chain = Chain::new();
    let mut coin = launch(&chain, Rules { target_start: 10, target_floor: 4, ..rules() });
    chain.next_round();
    chain.as_user("x.near");
    coin.settle();
    assert_eq!(coin.status().target, 7, "10 down 30%");
    chain.next_round();
    chain.as_user("x.near");
    coin.settle();
    assert_eq!(coin.status().target, 4, "7 down 30% is 4.9, rounded down to the floor of 4");
    chain.next_round();
    chain.as_user("x.near");
    coin.settle();
    assert_eq!(coin.status().target, 4, "held at the floor");
}

#[test]
fn a_streak_grows_the_meal_up_to_its_cap() {
    let mut chain = Chain::new();
    let mut coin = launch(&chain, Rules { target_start: 1, target_up_bps: 0, streak_cap: 2, ..rules() });
    register(&chain, &mut coin, "alice.near");
    let mut got = vec![];
    for _ in 0..5 {
        feed(&chain, &mut coin, "alice.near", "runt-a.poolv1.near", 100);
        chain.next_round();
        chain.as_user("alice.near");
        got.push(coin.claim().0);
    }
    let base = 800_000 * TOKEN;
    assert_eq!(got, vec![base, base * 102 / 100, base * 104 / 100, base * 104 / 100, base * 104 / 100]);
    assert_eq!(coin.status().best_streak, 5);
}

#[test]
fn an_idle_coin_closes_in_steps_and_folds_into_one_line() {
    let mut chain = Chain::new();
    let mut coin = launch(&chain, Rules { target_start: 1, ..rules() });
    register(&chain, &mut coin, "alice.near");
    feed(&chain, &mut coin, "alice.near", "runt-a.poolv1.near", 100);
    chain.now += DAY * 100;
    chain.as_user("alice.near");
    assert_eq!(coin.settle(), 100, "a long gap closes in one call");
    assert_eq!(coin.settle(), 0);
    let status = coin.status();
    assert_eq!(status.open.round, 100);
    assert_eq!(status.rounds_recorded, 2, "one line for the hit, one line for 99 empty rounds");
    let lines = coin.rounds(None, None);
    assert_eq!((lines[1].line.round, lines[1].line.through), (1, 99));
    assert_books_balance(&chain, &coin);
}

#[test]
fn feeding_after_a_long_gap_closes_it_first() {
    let mut chain = Chain::new();
    let mut coin = launch(&chain, rules());
    register(&chain, &mut coin, "alice.near");
    chain.now += DAY * 100;
    assert!(feed(&chain, &mut coin, "alice.near", "runt-a.poolv1.near", 100) > 0);
    assert_eq!(coin.status().open.round, 100);
}

#[test]
fn idle_rounds_spoil_exactly_like_rounds_closed_one_by_one() {
    let setup = |chain: &mut Chain| {
        let mut coin = launch(chain, Rules { target_start: 1, ..rules() });
        register(chain, &mut coin, "alice.near");
        feed(chain, &mut coin, "alice.near", "runt-a.poolv1.near", 100);
        coin
    };
    // Close the hit, then ten empty rounds in a single jump.
    let mut chain = Chain::new();
    let mut together = setup(&mut chain);
    chain.now += DAY * 11;
    chain.as_user("alice.near");
    together.settle();
    let (kennel_together, burned_together) = (kennel(&chain, &together), together.status().burned.0);
    drop(together);
    near_sdk::mock::with_mocked_blockchain(|b| drop(b.take_storage()));

    // The same, one round per call.
    let mut chain = Chain::new();
    let mut stepped = setup(&mut chain);
    for _ in 0..11 {
        chain.next_round();
        chain.as_user("alice.near");
        stepped.settle();
    }
    assert_eq!(kennel(&chain, &stepped), kennel_together);
    assert_eq!(stepped.status().burned.0, burned_together);
}

#[test]
fn a_coin_left_for_a_very_long_time_still_closes_in_one_call() {
    let mut chain = Chain::new();
    let mut coin = launch(&chain, Rules { target_start: 1, round_seconds: 60, ..rules() });
    register(&chain, &mut coin, "alice.near");
    feed(&chain, &mut coin, "alice.near", "runt-a.poolv1.near", 100);
    chain.now += 60 * NANOS_PER_SECOND * 2_000_000;
    chain.as_user("alice.near");
    assert_eq!(coin.settle(), 2_000_000);
    assert!(kennel(&chain, &coin) < 4, "everything but a few units of dust has spoiled");
    assert_books_balance(&chain, &coin);
}

// ---- The roll call ----------------------------------------------------------------------------

#[test]
fn a_roll_call_measures_the_exact_nakamoto_number() {
    let mut chain = Chain::new();
    // 1,000,000 NEAR in all. A third is 333,333, so the top validator alone is enough.
    let mut coin = launch(&chain, rules());
    chain.as_user("taker.near");
    let roll = coin.roll_call(chain.sorted());
    assert_eq!(roll.nakamoto, 1);
    assert_eq!(roll.validators, 7);
    assert_eq!(roll.halting_share_ppm, 400_000);
    chain.set(&[("a.poolv1.near", 300_000), ("b.poolv1.near", 300_000), ("c.poolv1.near", 200_000), ("d.poolv1.near", 200_000)]);
    chain.next_round();
    chain.as_user("taker.near");
    assert_eq!(coin.roll_call(chain.sorted()).nakamoto, 2);
    assert_eq!(coin.nakamoto().unwrap().nakamoto, 2);
}

#[test]
fn exactly_a_third_is_not_enough() {
    let mut chain = Chain::new();
    chain.set(&[("a.poolv1.near", 100), ("b.poolv1.near", 100), ("c.poolv1.near", 100)]);
    let coin = launch(&chain, rules());
    chain.as_user("x.near");
    assert_eq!(coin.check_roll_call(chain.sorted()).nakamoto, 2);
}

#[test]
fn effective_validators_is_right_for_an_equal_set() {
    let mut chain = Chain::new();
    chain.set(&[("a.poolv1.near", 100), ("b.poolv1.near", 100), ("c.poolv1.near", 100), ("d.poolv1.near", 100)]);
    let coin = launch(&chain, rules());
    chain.as_user("x.near");
    assert_eq!(coin.check_roll_call(chain.sorted()).effective_x100.0, 400);
}

#[test]
#[should_panic(expected = "RUNT[ROLL] the list is incomplete")]
fn leaving_out_a_validator_fails() {
    let chain = Chain::new();
    let coin = launch(&chain, rules());
    let mut list = chain.sorted();
    list.remove(1);
    chain.as_user("x.near");
    coin.check_roll_call(list);
}

#[test]
#[should_panic(expected = "is out of order")]
fn the_wrong_order_fails() {
    let chain = Chain::new();
    let coin = launch(&chain, rules());
    let mut list = chain.sorted();
    list.swap(0, 1);
    chain.as_user("x.near");
    coin.check_roll_call(list);
}

#[test]
#[should_panic(expected = "is out of order")]
fn listing_a_validator_twice_fails() {
    let chain = Chain::new();
    let coin = launch(&chain, rules());
    let mut list = chain.sorted();
    list.insert(1, list[0].clone());
    chain.as_user("x.near");
    coin.check_roll_call(list);
}

#[test]
#[should_panic(expected = "is not an active validator")]
fn a_stranger_in_the_list_fails() {
    let chain = Chain::new();
    let coin = launch(&chain, rules());
    let mut list = chain.sorted();
    list.push(id("stranger.near"));
    chain.as_user("x.near");
    coin.check_roll_call(list);
}

#[test]
fn equal_stakes_are_ordered_by_account() {
    let mut chain = Chain::new();
    chain.set(&[("b.poolv1.near", 100), ("a.poolv1.near", 100)]);
    let coin = launch(&chain, rules());
    chain.as_user("x.near");
    assert_eq!(coin.check_roll_call(vec![id("a.poolv1.near"), id("b.poolv1.near")]).validators, 2);
}

#[test]
#[should_panic(expected = "is out of order")]
fn equal_stakes_in_the_wrong_account_order_fail() {
    let mut chain = Chain::new();
    chain.set(&[("b.poolv1.near", 100), ("a.poolv1.near", 100)]);
    let coin = launch(&chain, rules());
    chain.as_user("x.near");
    coin.check_roll_call(vec![id("b.poolv1.near"), id("a.poolv1.near")]);
}

#[test]
#[should_panic(expected = "RUNT[COUNTED]")]
fn one_roll_call_per_round() {
    let chain = Chain::new();
    let mut coin = launch(&chain, rules());
    chain.as_user("x.near");
    coin.roll_call(chain.sorted());
    coin.roll_call(chain.sorted());
}

#[test]
fn the_roll_call_pays_its_bounty_from_the_kennel_once_it_can() {
    let mut chain = Chain::new();
    let mut coin = launch(&chain, Rules { target_start: 1, ..rules() });
    register(&chain, &mut coin, "taker.near");
    chain.as_user("taker.near");
    coin.roll_call(chain.sorted());
    assert_eq!(balance(&chain, &coin, "taker.near"), 0, "the Kennel is empty, so there is nothing to pay");
    register(&chain, &mut coin, "alice.near");
    feed(&chain, &mut coin, "alice.near", "runt-a.poolv1.near", 100);
    chain.next_round();
    chain.as_user("taker.near");
    coin.settle();
    let before = kennel(&chain, &coin);
    chain.as_user("taker.near");
    coin.roll_call(chain.sorted());
    assert_eq!(balance(&chain, &coin, "taker.near"), 5_000 * TOKEN);
    assert_eq!(kennel(&chain, &coin), before - 5_000 * TOKEN);
    assert_books_balance(&chain, &coin);
}

#[test]
fn the_first_roll_call_only_sets_the_baseline() {
    let mut chain = Chain::new();
    let mut coin = launch(&chain, Rules { target_start: 1, ..rules() });
    register(&chain, &mut coin, "alice.near");
    feed(&chain, &mut coin, "alice.near", "runt-a.poolv1.near", 100);
    chain.as_user("taker.near");
    coin.roll_call(chain.sorted());
    chain.next_round();
    chain.as_user("alice.near");
    assert_eq!(coin.claim().0, 800_000 * TOKEN, "no bonus: there was nothing to compare with");
    assert_eq!(coin.status().last_nakamoto, 1);
}

#[test]
fn when_the_number_rises_the_meal_triples() {
    let mut chain = Chain::new();
    let mut coin = launch(&chain, Rules { target_start: 1, baseline_nakamoto: 1, ..rules() });
    register(&chain, &mut coin, "alice.near");
    feed(&chain, &mut coin, "alice.near", "runt-a.poolv1.near", 100);
    chain.set(&[
        ("a.poolv1.near", 300_000),
        ("b.poolv1.near", 300_000),
        ("c.poolv1.near", 200_000),
        ("d.poolv1.near", 199_000),
        ("runt-a.poolv1.near", 1_000),
    ]);
    chain.as_user("taker.near");
    assert_eq!(coin.roll_call(chain.sorted()).nakamoto, 2);
    chain.next_round();
    chain.as_user("alice.near");
    assert_eq!(coin.claim().0, 2_400_000 * TOKEN, "1M meal, tripled, less the Kennel's 20%");
    let status = coin.status();
    assert_eq!((status.last_nakamoto, status.best_nakamoto), (2, 2));
}

#[test]
fn when_the_number_falls_half_the_kennel_burns() {
    let mut chain = Chain::new();
    let mut coin = launch(&chain, Rules { target_start: 1, baseline_nakamoto: 3, ..rules() });
    register(&chain, &mut coin, "alice.near");
    feed(&chain, &mut coin, "alice.near", "runt-a.poolv1.near", 100);
    chain.as_user("taker.near");
    coin.roll_call(chain.sorted());
    chain.next_round();
    chain.as_user("alice.near");
    coin.settle();
    let status = coin.status();
    assert_eq!(status.last_nakamoto, 1);
    assert_eq!(status.best_nakamoto, 3, "the record stays");
    assert_eq!(status.burned.0, 100_000 * TOKEN, "half of the 200,000 the Kennel just received");
    assert_eq!(kennel(&chain, &coin), 100_000 * TOKEN);
    assert_books_balance(&chain, &coin);
}

#[test]
fn a_round_without_a_roll_call_changes_nothing() {
    let mut chain = Chain::new();
    let mut coin = launch(&chain, Rules { target_start: 1, baseline_nakamoto: 9, ..rules() });
    chain.next_round();
    chain.as_user("x.near");
    coin.settle();
    let status = coin.status();
    assert_eq!((status.last_nakamoto, status.burned.0), (9, 0));
}

// ---- NEP-141 ----------------------------------------------------------------------------------

#[test]
fn it_is_a_normal_nep141_token() {
    let mut chain = Chain::new();
    let mut coin = launch(&chain, Rules { target_start: 1, ..rules() });
    for who in ["alice.near", "bob.near"] {
        register(&chain, &mut coin, who);
    }
    feed(&chain, &mut coin, "alice.near", "runt-a.poolv1.near", 100);
    chain.next_round();
    chain.as_user("alice.near");
    coin.claim();
    chain.call("alice.near", NearToken::from_yoctonear(1));
    coin.ft_transfer(id("bob.near"), U128(1_000 * TOKEN), None);
    assert_eq!(balance(&chain, &coin, "bob.near"), 1_000 * TOKEN);
    assert_eq!(balance(&chain, &coin, "alice.near"), 799_000 * TOKEN);
    let metadata = coin.ft_metadata();
    assert_eq!((metadata.symbol.as_str(), metadata.decimals, metadata.spec.as_str()), ("RUNT", 18, "ft-1.0.0"));
}

#[test]
#[should_panic(expected = "Requires attached deposit of exactly 1 yoctoNEAR")]
fn transfers_need_one_yocto() {
    let chain = Chain::new();
    let mut coin = launch(&chain, rules());
    chain.as_user("alice.near");
    coin.ft_transfer(id("bob.near"), U128(1), None);
}

#[test]
fn the_registration_fee_covers_the_plate() {
    let chain = Chain::new();
    let coin = launch(&chain, rules());
    chain.as_user("x.near");
    let fee = coin.storage_balance_bounds().min;
    let plain = NearToken::from_yoctonear(u128::from(125u64) * env::storage_byte_cost().as_yoctonear());
    assert!(fee > plain, "fee {fee} should be more than a plain token registration");
}

#[test]
fn leaving_with_force_burns_the_balance_and_clears_the_plate() {
    let mut chain = Chain::new();
    let mut coin = launch(&chain, Rules { target_start: 1, ..rules() });
    register(&chain, &mut coin, "alice.near");
    feed(&chain, &mut coin, "alice.near", "runt-a.poolv1.near", 100);
    chain.next_round();
    chain.as_user("alice.near");
    coin.claim();
    feed(&chain, &mut coin, "alice.near", "runt-a.poolv1.near", 100);
    chain.next_round();
    chain.call("alice.near", NearToken::from_yoctonear(1));
    assert!(coin.storage_unregister(Some(true)));
    assert!(coin.plate_of(id("alice.near")).is_none());
    assert_eq!(coin.status().burned.0, coin.status().minted.0 - coin.status().total_supply.0);
    assert_books_balance(&chain, &coin);
}

#[test]
#[should_panic(expected = "RUNT[FED]")]
fn leaving_and_coming_back_cannot_feed_twice_in_one_round() {
    let chain = Chain::new();
    let mut coin = launch(&chain, rules());
    register(&chain, &mut coin, "mallory.near");
    feed(&chain, &mut coin, "mallory.near", "runt-a.poolv1.near", 1_000);
    chain.call("mallory.near", NearToken::from_yoctonear(1));
    coin.storage_unregister(None);
}

#[test]
#[should_panic(expected = "RUNT[FED]")]
fn leaving_with_a_feed_still_in_flight_is_refused() {
    let chain = Chain::new();
    let mut coin = launch(&chain, rules());
    register(&chain, &mut coin, "mallory.near");
    chain.as_user("mallory.near");
    let _ = coin.feed(id("runt-a.poolv1.near"));
    chain.call("mallory.near", NearToken::from_yoctonear(1));
    coin.storage_unregister(None);
}

#[test]
fn an_answer_nothing_is_waiting_for_counts_for_nothing() {
    let chain = Chain::new();
    let mut coin = launch(&chain, rules());
    register(&chain, &mut coin, "alice.near");
    chain.as_contract();
    assert_eq!(coin.on_feed(id("nobody.near"), id("runt-a.poolv1.near"), 0, 10_000, Ok(U128(5_000 * ONE_NEAR))).0, 0);
    assert_eq!(coin.status().open.bites.0, 0);
    let bite = feed(&chain, &mut coin, "alice.near", "runt-a.poolv1.near", 100);
    chain.as_contract();
    assert_eq!(coin.on_feed(id("alice.near"), id("runt-a.poolv1.near"), 0, 10_000, Ok(U128(5_000 * ONE_NEAR))).0, 0, "a repeated answer");
    let status = coin.status();
    assert_eq!((status.open.bites.0, status.open.feeders), (bite, 1));
}

#[test]
fn an_answer_for_a_different_pool_is_refused() {
    let chain = Chain::new();
    let mut coin = launch(&chain, rules());
    register(&chain, &mut coin, "alice.near");
    chain.as_user("alice.near");
    let _ = coin.feed(id("runt-a.poolv1.near"));
    chain.as_contract();
    assert_eq!(coin.on_feed(id("alice.near"), id("runt-b.pool.near"), 0, 10_000, Ok(U128(100 * ONE_NEAR))).0, 0);
    assert_eq!(coin.status().open.plates, 0);
}

#[test]
#[should_panic(expected = "Can't unregister")]
fn a_plain_exit_collects_the_meal_first_and_then_needs_force() {
    let mut chain = Chain::new();
    let mut coin = launch(&chain, Rules { target_start: 1, ..rules() });
    register(&chain, &mut coin, "alice.near");
    feed(&chain, &mut coin, "alice.near", "runt-a.poolv1.near", 100);
    chain.next_round();
    chain.call("alice.near", NearToken::from_yoctonear(1));
    coin.storage_unregister(None);
}

#[test]
#[should_panic(expected = "RUNT[KENNEL]")]
fn tokens_cannot_be_parked_on_the_kennel() {
    let chain = Chain::new();
    let mut coin = launch(&chain, rules());
    chain.call("alice.near", NearToken::from_yoctonear(1));
    let _ = coin.ft_transfer_call(id("runt.near"), U128(1), None, String::new());
}

#[test]
fn a_plain_donation_to_the_kennel_still_works() {
    let mut chain = Chain::new();
    let mut coin = launch(&chain, Rules { target_start: 1, ..rules() });
    register(&chain, &mut coin, "alice.near");
    feed(&chain, &mut coin, "alice.near", "runt-a.poolv1.near", 100);
    chain.next_round();
    chain.as_user("alice.near");
    coin.claim();
    let before = kennel(&chain, &coin);
    chain.call("alice.near", NearToken::from_yoctonear(1));
    coin.ft_transfer(id("runt.near"), U128(10 * TOKEN), None);
    assert_eq!(kennel(&chain, &coin), before + 10 * TOKEN);
    assert_books_balance(&chain, &coin);
}

#[test]
fn a_burn_during_a_refund_is_reported_once() {
    let mut chain = Chain::new();
    let mut coin = launch(&chain, Rules { target_start: 1, ..rules() });
    for who in ["alice.near", "bob.near"] {
        register(&chain, &mut coin, who);
    }
    feed(&chain, &mut coin, "alice.near", "runt-a.poolv1.near", 100);
    chain.next_round();
    chain.as_user("alice.near");
    coin.claim();
    let amount = 800_000 * TOKEN;
    chain.call("alice.near", NearToken::from_yoctonear(1));
    let _ = coin.ft_transfer_call(id("bob.near"), U128(amount), None, String::new());
    chain.call("alice.near", NearToken::from_yoctonear(1));
    assert!(coin.storage_unregister(None));
    chain.call_with_results("runt.near", NearToken::from_yoctonear(0), vec![PromiseResult::Failed]);
    coin.ft_resolve_transfer(id("alice.near"), id("bob.near"), U128(amount));
    let burns = get_logs().iter().filter(|l| l.contains("\"event\":\"ft_burn\"")).count();
    assert_eq!(burns, 1);
    assert_books_balance(&chain, &coin);
}

#[test]
fn the_kennel_mint_is_logged_under_the_round_that_earned_it() {
    let mut chain = Chain::new();
    let mut coin = launch(&chain, Rules { target_start: 1, ..rules() });
    register(&chain, &mut coin, "alice.near");
    feed(&chain, &mut coin, "alice.near", "runt-a.poolv1.near", 100);
    chain.now += DAY * 6;
    chain.as_user("alice.near");
    coin.settle();
    let line = coin.log(None).into_iter().find(|l| l.kind == LogKind::Kennel).unwrap();
    assert_eq!(line.round, 0);
}

#[test]
#[should_panic(expected = "the biggest possible meal is too large")]
fn rules_that_could_jam_the_coin_one_day_are_refused() {
    let chain = Chain::new();
    launch(&chain, Rules { meal: U128(1_000_000_000 * TOKEN), streak_step_bps: 100_000, streak_cap: u32::MAX, ..rules() });
}

// ---- Rules and books --------------------------------------------------------------------------

#[test]
fn every_rule_is_readable() {
    let chain = Chain::new();
    let coin = launch(&chain, rules());
    chain.as_user("anyone.near");
    assert_eq!(coin.rules(), rules());
}

#[test]
#[should_panic(expected = "RUNT[RULES]")]
fn impossible_rules_are_refused() {
    let chain = Chain::new();
    launch(&chain, Rules { spoil_bps: 10_001, ..rules() });
}

#[test]
fn the_log_keeps_the_latest_lines() {
    let mut chain = Chain::new();
    let mut coin = launch(&chain, Rules { target_start: 1, target_up_bps: 0, ..rules() });
    register(&chain, &mut coin, "alice.near");
    for _ in 0..80 {
        feed(&chain, &mut coin, "alice.near", "runt-a.poolv1.near", 100);
        chain.next_round();
        chain.as_user("alice.near");
        coin.claim();
    }
    assert_eq!(coin.status().log_total.0, 160, "one Kennel mint and one meal per round");
    let log = coin.log(Some(500));
    assert_eq!(log.len() as u64, LOG_SIZE);
    assert_eq!(log[0].kind, LogKind::Meal);
    assert_eq!(log[0].round, 79);
    assert_books_balance(&chain, &coin);
}

/// Random actions over many rounds: the books must always balance, and no call may panic.
#[test]
fn the_books_balance_through_a_long_messy_run() {
    let mut chain = Chain::new();
    let mut coin = launch(&chain, Rules { target_start: 2, target_floor: 1, ..rules() });
    let people = ["a.near", "b.near", "c.near", "d.near", "e.near"];
    let pools = ["runt-a.poolv1.near", "runt-b.pool.near", "runt-c.poolv1.near"];
    for who in people {
        register(&chain, &mut coin, who);
    }
    let mut seed: u64 = 7;
    let mut next = |limit: u64| {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (seed >> 33) % limit
    };
    for _ in 0..60 {
        for who in people {
            chain.as_user(who);
            coin.claim();
            if coin.plate_of(id(who)).is_none() && next(10) < 6 {
                let stake = 10 + u128::from(next(5_000));
                feed(&chain, &mut coin, who, pools[next(3) as usize], stake);
            }
        }
        if next(10) < 3 {
            chain.as_user("taker.near");
            if coin.status().open.nakamoto == 0 {
                coin.roll_call(chain.sorted());
            }
        }
        chain.now += DAY * (1 + next(3));
        chain.as_user("taker.near");
        coin.settle();
        assert_books_balance(&chain, &coin);
        chain.as_user("taker.near");
    }
    let status = coin.status();
    let mut held = kennel(&chain, &coin);
    for who in people {
        held += balance(&chain, &coin, who);
    }
    assert_eq!(held, status.total_supply.0, "every token is held by someone");
    assert!(status.best_streak >= 2 && status.minted.0 > 0 && status.burned.0 > 0, "the run should exercise hits and misses");
}
