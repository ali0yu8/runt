//! RUNT: a NEP-141 memecoin that can only be earned by keeping NEAR delegated to a small validator.
//!
//! Nothing here holds anyone's NEAR. `feed` reads a delegation straight from the staking pool, and
//! `roll_call` measures NEAR's Nakamoto coefficient from the protocol's own validator numbers, so
//! there is no oracle, no keeper and no owner. Every rule is fixed in `new`.

use near_contract_standards::fungible_token::FungibleToken;
use near_contract_standards::fungible_token::core::FungibleTokenCore;
use near_contract_standards::fungible_token::events::{FtBurn, FtMint};
use near_contract_standards::fungible_token::metadata::{
    FT_METADATA_SPEC, FungibleTokenMetadata, FungibleTokenMetadataProvider,
};
use near_contract_standards::fungible_token::resolver::FungibleTokenResolver;
use near_contract_standards::storage_management::{
    StorageBalance, StorageBalanceBounds, StorageManagement,
};
use near_sdk::json_types::{U64, U128};
use near_sdk::serde_json::{Value, json};
use near_sdk::store::{LookupMap, Vector};
use near_sdk::{
    AccountId, BorshStorageKey, Gas, NearToken, PanicOnDefault, Promise, PromiseError,
    PromiseOrValue, env, ext_contract, near, require,
};

const BPS: u128 = 10_000;
const ONE_NEAR: u128 = 1_000_000_000_000_000_000_000_000;
const MILLION: u128 = 1_000_000;
const TRILLION: u128 = 1_000_000_000_000;
const NANOS_PER_SECOND: u64 = 1_000_000_000;

/// How many mints and burns the on-chain log keeps. All of them also stay in the chain's event log.
const LOG_SIZE: u64 = 128;
/// How many rounds of decay one call applies to an idle coin, so the call stays cheap however long
/// the coin was left. At the launch spoil rate of 25% the Kennel is gone long before this. A coin
/// launched with a very low spoil rate would burn less than it should across a very long gap.
const MAX_IDLE_STEPS: u32 = 256;
/// Every registered account can hold one plate, so the registration fee pays for it up front.
const PLATE_BYTES: u64 = 200;

const GAS_READ_POOL: Gas = Gas::from_tgas(6);
const GAS_ON_FEED: Gas = Gas::from_tgas(14);

const ICON: &str = "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 64 64'%3E%3Ccircle cx='32' cy='32' r='32' fill='%23ffb627'/%3E%3Cpath d='M14 24c-5-3-7-11-3-15 6 0 12 4 15 9zM50 24c5-3 7-11 3-15-6 0-12 4-15 9z' fill='%237a3e14'/%3E%3Cellipse cx='32' cy='36' rx='20' ry='18' fill='%23a65a24'/%3E%3Cellipse cx='32' cy='44' rx='11' ry='9' fill='%23ffe3b8'/%3E%3Ccircle cx='24' cy='32' r='3.4' fill='%23201008'/%3E%3Ccircle cx='40' cy='32' r='3.4' fill='%23201008'/%3E%3Cellipse cx='32' cy='40' rx='4' ry='3' fill='%23201008'/%3E%3C/svg%3E";

// ---- Small helpers --------------------------------------------------------------------------

uint::construct_uint! {
    /// Only for the multiply-then-divide steps, where the middle value can pass 128 bits.
    pub struct U256(4);
}

fn mul_div(a: u128, b: u128, c: u128) -> u128 {
    if c == 0 {
        return 0;
    }
    let value = U256::from(a) * U256::from(b) / U256::from(c);
    if value > U256::from(u128::MAX) { u128::MAX } else { value.low_u128() }
}

fn isqrt(n: u128) -> u128 {
    if n < 2 {
        return n;
    }
    let mut x = 1u128 << ((128 - n.leading_zeros()).div_ceil(2));
    loop {
        let y = (x + n / x) / 2;
        if y >= x {
            return x;
        }
        x = y;
    }
}

/// How hungry a validator is, in basis points: full for a validator with almost no stake, and
/// falling to zero at the size cap. It reads the protocol's stake figures for this epoch.
pub fn hunger_bps(validator_stake: u128, total_stake: u128, size_cap_bps: u32) -> u32 {
    if total_stake == 0 {
        return 0;
    }
    let share_ppm = mul_div(validator_stake, MILLION, total_stake);
    let cap_ppm = u128::from(size_cap_bps) * 100;
    if share_ppm >= cap_ppm {
        return 0;
    }
    let gap = cap_ppm - share_ppm;
    (BPS * gap * gap / (cap_ppm * cap_ppm)) as u32
}

/// The curve a runt climbs each round, in millionths: linear up to `full_near`, then a square root.
fn curve_micro(fed_near: u128, full_near: u128) -> u128 {
    if fed_near <= full_near {
        mul_div(fed_near, TRILLION, isqrt(full_near * TRILLION).max(1))
    } else {
        isqrt(fed_near * TRILLION)
    }
}

/// A feeder's points: how far they lift this runt up the curve, times its hunger.
/// Splitting between accounts on one runt changes nothing, because the steps add up to the same climb.
pub fn bite_micro(hunger: u32, fed_before: u128, stake_near: u128, full_near: u128) -> u128 {
    let climb = curve_micro(fed_before + stake_near, full_near) - curve_micro(fed_before, full_near);
    mul_div(u128::from(hunger), climb, BPS)
}

fn event(name: &str, data: Value) {
    let line = json!({ "standard": "runt", "version": "1.0.0", "event": name, "data": [data] });
    env::log_str(&format!("EVENT_JSON:{line}"));
}

#[ext_contract(ext_pool)]
#[allow(dead_code)]
trait StakingPool {
    fn get_account_staked_balance(&self, account_id: AccountId) -> U128;
}

// ---- Rules ----------------------------------------------------------------------------------

/// Everything a launch decides. Set once in `new`, returned by `rules`, with no way to change it.
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub struct Rules {
    /// Length of a round. 86,400 on the main coin, shorter on demo coins.
    pub round_seconds: u32,
    /// RUNT (yocto-style, 18 decimals) made by a hit round before any bonus.
    pub meal: U128,
    /// Extra meal per round of streak, in basis points.
    pub streak_step_bps: u32,
    /// The streak bonus stops growing at this many rounds.
    pub streak_cap: u32,
    /// Share of each meal kept in the Kennel.
    pub kennel_bps: u32,
    /// Share of the Kennel burned when a round misses its target.
    pub spoil_bps: u32,
    /// Different runts the pack must feed in the first round.
    pub target_start: u32,
    /// The target can never fall below this.
    pub target_floor: u32,
    /// Target growth after a hit, rounded up.
    pub target_up_bps: u32,
    /// Target fall after a miss, rounded down.
    pub target_down_bps: u32,
    /// Validators holding at least this share of all stake have no hunger and can't be fed.
    pub size_cap_bps: u32,
    /// The least delegated stake that can feed, in whole NEAR.
    pub min_stake_near: u32,
    /// NEAR per runt per round that counts in full. A square root applies above it.
    pub full_bite_near: u32,
    /// Paid from the Kennel for the first complete roll call of each round.
    pub census_bounty: U128,
    /// Extra meal in a round where the Nakamoto number rose, in basis points.
    pub nakamoto_up_bonus_bps: u32,
    /// Share of the Kennel burned in a round where the Nakamoto number fell.
    pub nakamoto_down_burn_bps: u32,
    /// The number the first roll call is compared with. Zero means the first one only sets it.
    pub baseline_nakamoto: u32,
    /// Staking-pool factories whose pools can be fed, such as `poolv1.near`.
    pub pool_suffixes: Vec<String>,
}

impl Rules {
    fn check(&self) {
        let is_bps = |v: u32| u128::from(v) <= BPS;
        require!(self.round_seconds >= 60, "RUNT[RULES] rounds must be at least 60 seconds");
        require!(self.meal.0 > 0 && self.meal.0 <= 1_000_000_000 * 10u128.pow(18), "RUNT[RULES] meal out of range");
        require!(self.target_floor >= 1, "RUNT[RULES] the target floor must be at least 1");
        require!(self.target_start >= self.target_floor, "RUNT[RULES] the target must start at or above its floor");
        require!(
            is_bps(self.kennel_bps) && is_bps(self.spoil_bps) && is_bps(self.target_down_bps) && is_bps(self.nakamoto_down_burn_bps),
            "RUNT[RULES] a share is above 100%"
        );
        require!(self.size_cap_bps >= 1 && is_bps(self.size_cap_bps), "RUNT[RULES] size cap out of range");
        require!(self.min_stake_near >= 1 && self.full_bite_near >= 1, "RUNT[RULES] stake limits must be at least 1 NEAR");
        require!(!self.pool_suffixes.is_empty(), "RUNT[RULES] name at least one staking pool factory");
        require!(self.streak_step_bps <= 100_000 && self.nakamoto_up_bonus_bps <= 1_000_000, "RUNT[RULES] bonus out of range");
        // A meal so large that the supply could overflow would jam a coin nobody can fix.
        let longest_streak = BPS + u128::from(self.streak_cap) * u128::from(self.streak_step_bps);
        let worst = mul_div(mul_div(self.meal.0, longest_streak, BPS), BPS + u128::from(self.nakamoto_up_bonus_bps), BPS);
        require!(worst < 1u128 << 96, "RUNT[RULES] the biggest possible meal is too large");
    }
}

// ---- State ----------------------------------------------------------------------------------

#[near]
#[derive(BorshStorageKey)]
enum Key {
    Balances,
    Fed,
    Plates,
    Meals,
    Rounds,
    Log,
}

/// The round still taking feeds.
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, Default)]
pub struct OpenRound {
    pub round: u32,
    /// Different runts fed so far.
    pub plates: u32,
    pub feeders: u32,
    pub bites: U128,
    /// This round's roll call, or zero if nobody has taken it.
    pub nakamoto: u32,
}

#[near(serializers = [borsh, json])]
#[derive(Clone, Debug)]
pub struct Plate {
    pub round: u32,
    pub bite: U128,
    pub pool: AccountId,
    /// True from `feed` until the pool has answered.
    pub pending: bool,
}

#[near(serializers = [borsh])]
#[derive(Clone, Debug)]
struct FedRunt {
    round: u32,
    near: u128,
}

#[near(serializers = [borsh, json])]
#[derive(Clone, Debug)]
pub struct Meal {
    /// What feeders share. The Kennel's cut has already been taken.
    pub pot: U128,
    pub bites: U128,
    pub claimed: U128,
}

/// One closed round. Rounds nobody fed are folded into a single line.
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug)]
pub struct RoundLine {
    pub round: u32,
    /// The last round this line covers.
    pub through: u32,
    pub hit: bool,
    pub plates: u32,
    pub target: u32,
    pub feeders: u32,
    pub bites: U128,
    pub pot: U128,
    pub kennel_in: U128,
    pub burned: U128,
    pub streak: u32,
    pub nakamoto: u32,
}

#[near(serializers = [borsh, json])]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LogKind {
    /// A feeder's share of a meal, minted when claimed.
    Meal,
    /// The Kennel's cut of a meal, minted when a round closes.
    Kennel,
    /// Kennel burned after a missed round.
    Spoil,
    /// Kennel burned after the Nakamoto number fell.
    Goliath,
    /// Paid from the Kennel for a roll call. Moves tokens; mints and burns nothing.
    Bounty,
    /// Balance burned by an account leaving with `force`.
    Unregister,
}

#[near(serializers = [borsh, json])]
#[derive(Clone, Debug)]
pub struct LogLine {
    pub kind: LogKind,
    pub round: u32,
    pub account: AccountId,
    pub amount: U128,
    pub at: U64,
}

#[near(serializers = [borsh, json])]
#[derive(Clone, Debug)]
pub struct Census {
    pub round: u32,
    pub epoch: U64,
    pub nakamoto: u32,
    pub validators: u32,
    /// Effective number of validators (one over the sum of squared shares), times 100.
    pub effective_x100: U64,
    /// Share of all stake held by the validators counted in `nakamoto`, in parts per million.
    pub halting_share_ppm: u32,
    pub taken_by: AccountId,
}

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct Runt {
    token: FungibleToken,
    rules: Rules,
    started_at: u64,
    open: OpenRound,
    target: u32,
    streak: u32,
    best_streak: u32,
    last_nakamoto: u32,
    best_nakamoto: u32,
    last_census: Option<Census>,
    minted: u128,
    burned: u128,
    fed: LookupMap<AccountId, FedRunt>,
    plates: LookupMap<AccountId, Plate>,
    meals: LookupMap<u32, Meal>,
    rounds: Vector<RoundLine>,
    log: Vector<LogLine>,
    log_total: u64,
}

// ---- Things the page reads ------------------------------------------------------------------

#[near(serializers = [json])]
pub struct Status {
    /// The round the clock is in. If it is ahead of `open.round`, someone has yet to close the gap.
    pub clock_round: u32,
    pub open: OpenRound,
    pub started_at: U64,
    pub round_ends_at: U64,
    pub target: u32,
    pub streak: u32,
    pub best_streak: u32,
    pub kennel: U128,
    pub total_supply: U128,
    pub minted: U128,
    pub burned: U128,
    pub last_nakamoto: u32,
    pub best_nakamoto: u32,
    pub census: Option<Census>,
    pub rounds_recorded: u32,
    pub log_total: U64,
}

#[near(serializers = [json])]
pub struct Hunger {
    pub can_feed: bool,
    pub why_not: Option<String>,
    pub stake: U128,
    pub share_ppm: u32,
    pub hunger_bps: u32,
    pub fed_near_this_round: U128,
}

#[near(serializers = [json])]
pub struct Roll {
    pub nakamoto: u32,
    pub validators: u32,
    pub effective_x100: U64,
    pub halting_share_ppm: u32,
}

#[near(serializers = [json])]
pub struct RoundView {
    pub line: RoundLine,
    /// How much of this round's pot has been claimed so far.
    pub claimed: U128,
}

// ---- The contract ---------------------------------------------------------------------------

#[near]
impl Runt {
    /// Sets every rule, once. After this the deployer removes the account's keys, and nothing can
    /// change the rules or the code.
    #[init]
    pub fn new(rules: Rules) -> Self {
        rules.check();
        let mut token = FungibleToken::new(Key::Balances);
        // The registration fee also covers this account's plate.
        token.account_storage_usage += PLATE_BYTES;
        // The Kennel is this contract's own token balance.
        token.internal_register_account(&env::current_account_id());
        let target = rules.target_start;
        let baseline = rules.baseline_nakamoto;
        Self {
            token,
            rules,
            started_at: env::block_timestamp(),
            open: OpenRound::default(),
            target,
            streak: 0,
            best_streak: 0,
            last_nakamoto: baseline,
            best_nakamoto: baseline,
            last_census: None,
            minted: 0,
            burned: 0,
            fed: LookupMap::new(Key::Fed),
            plates: LookupMap::new(Key::Plates),
            meals: LookupMap::new(Key::Meals),
            rounds: Vector::new(Key::Rounds),
            log: Vector::new(Key::Log),
            log_total: 0,
        }
    }

    // ---- Actions ----------------------------------------------------------------------------

    /// Prove your NEAR is delegated to a small validator, and take a place at this round's meal.
    /// The contract only reads the pool. It never holds, moves or locks your NEAR.
    pub fn feed(&mut self, pool: AccountId) -> Promise {
        self.close_rounds();
        let who = env::predecessor_account_id();
        require!(self.token.accounts.contains_key(&who), "RUNT[REGISTER] call storage_deposit first");
        self.pay_plate(&who);
        let round = self.open.round;
        require!(!self.plates.contains_key(&who), "RUNT[FED] one feed per account per round");
        require!(self.pool_allowed(&pool), "RUNT[POOL] not a pool from an accepted staking factory");
        let stake = env::validator_stake(&pool).as_yoctonear();
        require!(stake > 0, "RUNT[ASLEEP] that validator is not in the active set this epoch");
        let hunger = hunger_bps(stake, env::validator_total_stake().as_yoctonear(), self.rules.size_cap_bps);
        require!(hunger > 0, "RUNT[BIGDOG] that validator is at or above the size cap");

        // Mark the plate before the outside call, so a second feed can't slip in while it waits.
        self.plates.insert(who.clone(), Plate { round, bite: U128(0), pool: pool.clone(), pending: true });
        ext_pool::ext(pool.clone())
            .with_static_gas(GAS_READ_POOL)
            .get_account_staked_balance(who.clone())
            .then(Self::ext(env::current_account_id()).with_static_gas(GAS_ON_FEED).on_feed(who, pool, round, hunger))
    }

    /// The staking pool's answer. Whatever goes wrong here, the plate is cleared so the feeder can try again.
    #[private]
    pub fn on_feed(
        &mut self,
        who: AccountId,
        pool: AccountId,
        round: u32,
        hunger: u32,
        #[callback_result] answer: Result<U128, PromiseError>,
    ) -> U128 {
        // Only the feed that marked this plate may fill it, and only once.
        let waiting = self.plates.get(&who).is_some_and(|p| p.pending && p.round == round && p.pool == pool);
        let reason = match &answer {
            _ if !waiting => Some("no feed is waiting for this answer"),
            Err(_) => Some("the pool did not answer"),
            Ok(_) if self.open.round != round => Some("the round ended first"),
            Ok(U128(stake)) if stake / ONE_NEAR < u128::from(self.rules.min_stake_near) => Some("not enough stake delegated"),
            _ => None,
        };
        if let Some(reason) = reason {
            self.drop_pending_plate(&who, round);
            event("feed_refused", json!({ "account": who, "pool": pool, "round": round, "reason": reason }));
            return U128(0);
        }
        let stake_near = answer.map(|v| v.0 / ONE_NEAR).unwrap_or(0);
        let before = self.fed.get(&pool).filter(|f| f.round == round).map_or(0, |f| f.near);
        let bite = bite_micro(hunger, before, stake_near, u128::from(self.rules.full_bite_near));
        if before == 0 {
            self.open.plates += 1;
        }
        self.open.feeders += 1;
        self.open.bites = U128(self.open.bites.0 + bite);
        self.fed.insert(pool.clone(), FedRunt { round, near: before + stake_near });
        self.plates.insert(who.clone(), Plate { round, bite: U128(bite), pool: pool.clone(), pending: false });
        event("fed", json!({
            "account": who, "pool": pool, "round": round, "stake_near": U128(stake_near),
            "hunger_bps": hunger, "bite": U128(bite), "plates": self.open.plates,
        }));
        U128(bite)
    }

    /// Collect a meal from a round that has ended. Feeding again collects it too.
    pub fn claim(&mut self) -> U128 {
        self.close_rounds();
        U128(self.pay_plate(&env::predecessor_account_id()))
    }

    /// Close any rounds whose time is up. Anyone can call this, and it needs no reward: every other
    /// action does it too. It returns how many rounds it closed.
    pub fn settle(&mut self) -> u32 {
        self.close_rounds()
    }

    /// Take this round's roll call. Pass every active validator, largest stake first, with ties in
    /// account order. The strict order rules out duplicates, and the total must match the protocol's
    /// to the last yoctoNEAR, so nobody can leave out a big validator. The result is exact.
    pub fn roll_call(&mut self, validators: Vec<AccountId>) -> Roll {
        self.close_rounds();
        require!(self.open.nakamoto == 0, "RUNT[COUNTED] this round's roll call has been taken");
        let roll = Self::count(&validators);
        let who = env::predecessor_account_id();
        self.open.nakamoto = roll.nakamoto;
        self.last_census = Some(Census {
            round: self.open.round,
            epoch: U64(env::epoch_height()),
            nakamoto: roll.nakamoto,
            validators: roll.validators,
            effective_x100: roll.effective_x100,
            halting_share_ppm: roll.halting_share_ppm,
            taken_by: who.clone(),
        });
        let here = env::current_account_id();
        let paid = self.rules.census_bounty.0.min(self.kennel());
        if paid > 0 && self.token.accounts.contains_key(&who) {
            self.token.internal_transfer(&here, &who, paid, Some("roll call bounty".to_string()));
            self.push_log(LogKind::Bounty, self.open.round, who.clone(), paid);
        }
        event("roll_call", json!({
            "round": self.open.round, "account": who, "nakamoto": roll.nakamoto,
            "validators": roll.validators, "effective_x100": roll.effective_x100,
        }));
        roll
    }

    // ---- Views ------------------------------------------------------------------------------

    pub fn rules(&self) -> Rules {
        self.rules.clone()
    }

    pub fn status(&self) -> Status {
        let clock = self.clock_round();
        Status {
            clock_round: clock,
            open: self.open.clone(),
            started_at: U64(self.started_at),
            round_ends_at: U64(self.started_at + (u64::from(clock) + 1) * self.round_nanos()),
            target: self.target,
            streak: self.streak,
            best_streak: self.best_streak,
            kennel: U128(self.kennel()),
            total_supply: self.token.ft_total_supply(),
            minted: U128(self.minted),
            burned: U128(self.burned),
            last_nakamoto: self.last_nakamoto,
            best_nakamoto: self.best_nakamoto,
            census: self.last_census.clone(),
            rounds_recorded: self.rounds.len(),
            log_total: U64(self.log_total),
        }
    }

    pub fn plate_of(&self, account_id: AccountId) -> Option<Plate> {
        self.plates.get(&account_id).cloned()
    }

    /// What this account could collect now. It counts a round whose time is up but hasn't been closed.
    pub fn claimable(&self, account_id: AccountId) -> U128 {
        let Some(plate) = self.plates.get(&account_id) else { return U128(0) };
        if plate.pending {
            return U128(0);
        }
        if plate.round < self.open.round {
            return U128(self.meals.get(&plate.round).map_or(0, |meal| mul_div(meal.pot.0, plate.bite.0, meal.bites.0)));
        }
        if self.clock_round() > self.open.round && self.open.plates >= self.target {
            let meal = self.meal_for(self.streak, self.nakamoto_rose());
            let pot = meal - mul_div(meal, u128::from(self.rules.kennel_bps), BPS);
            return U128(mul_div(pot, plate.bite.0, self.open.bites.0));
        }
        U128(0)
    }

    /// Whether a validator can be fed right now, and how hungry it is.
    pub fn hunger_of(&self, pool: AccountId) -> Hunger {
        self.hunger_view(&pool, env::validator_total_stake().as_yoctonear())
    }

    /// The same for many validators at once, so a page needs only one read.
    pub fn hungers(&self, pools: Vec<AccountId>) -> Vec<Hunger> {
        let total = env::validator_total_stake().as_yoctonear();
        pools.iter().map(|pool| self.hunger_view(pool, total)).collect()
    }

    fn hunger_view(&self, pool: &AccountId, total: u128) -> Hunger {
        let stake = env::validator_stake(pool).as_yoctonear();
        let hunger = hunger_bps(stake, total, self.rules.size_cap_bps);
        let why_not = if !self.pool_allowed(pool) {
            Some("not from an accepted staking pool factory")
        } else if stake == 0 {
            Some("not in the active validator set this epoch")
        } else if hunger == 0 {
            Some("at or above the size cap")
        } else {
            None
        };
        let fed = self.fed.get(pool).filter(|f| f.round == self.open.round).map_or(0, |f| f.near);
        Hunger {
            can_feed: why_not.is_none(),
            why_not: why_not.map(str::to_string),
            stake: U128(stake),
            share_ppm: mul_div(stake, MILLION, total) as u32,
            hunger_bps: hunger,
            fed_near_this_round: U128(fed),
        }
    }

    /// Checks a validator list without recording anything. Any contract or person can use this as a
    /// Nakamoto coefficient that needs no oracle.
    pub fn check_roll_call(&self, validators: Vec<AccountId>) -> Roll {
        Self::count(&validators)
    }

    /// The last recorded measurement of how many validators hold more than a third of NEAR's stake.
    pub fn nakamoto(&self) -> Option<Census> {
        self.last_census.clone()
    }

    pub fn rounds(&self, from: Option<u32>, limit: Option<u32>) -> Vec<RoundView> {
        let start = from.unwrap_or(0);
        let take = limit.unwrap_or(50).min(200);
        (start..self.rounds.len().min(start.saturating_add(take)))
            .filter_map(|i| self.rounds.get(i).cloned())
            .map(|line| {
                let claimed = self.meals.get(&line.round).map_or(U128(0), |meal| meal.claimed);
                RoundView { line, claimed }
            })
            .collect()
    }

    /// The most recent mints and burns, newest first.
    pub fn log(&self, limit: Option<u32>) -> Vec<LogLine> {
        let take = u64::from(limit.unwrap_or(50)).min(LOG_SIZE).min(self.log_total);
        (0..take)
            .filter_map(|i| {
                let slot = ((self.log_total - 1 - i) % LOG_SIZE) as u32;
                self.log.get(slot).cloned()
            })
            .collect()
    }

    // ---- Inside ------------------------------------------------------------------------------

    fn round_nanos(&self) -> u64 {
        u64::from(self.rules.round_seconds) * NANOS_PER_SECOND
    }

    fn clock_round(&self) -> u32 {
        ((env::block_timestamp().saturating_sub(self.started_at)) / self.round_nanos()) as u32
    }

    fn kennel(&self) -> u128 {
        self.token.accounts.get(&env::current_account_id()).unwrap_or(0)
    }

    fn pool_allowed(&self, pool: &AccountId) -> bool {
        let name = pool.as_str();
        self.rules.pool_suffixes.iter().any(|suffix| {
            name.len() > suffix.len() + 1 && name.ends_with(suffix.as_str()) && name.as_bytes()[name.len() - suffix.len() - 1] == b'.'
        })
    }

    fn nakamoto_rose(&self) -> bool {
        self.open.nakamoto > 0 && self.last_nakamoto > 0 && self.open.nakamoto > self.last_nakamoto
    }

    fn nakamoto_fell(&self) -> bool {
        self.open.nakamoto > 0 && self.last_nakamoto > 0 && self.open.nakamoto < self.last_nakamoto
    }

    fn meal_for(&self, streak: u32, nakamoto_rose: bool) -> u128 {
        let steps = u128::from(streak.min(self.rules.streak_cap));
        let meal = mul_div(self.rules.meal.0, BPS + steps * u128::from(self.rules.streak_step_bps), BPS);
        if nakamoto_rose { mul_div(meal, BPS + u128::from(self.rules.nakamoto_up_bonus_bps), BPS) } else { meal }
    }

    fn drop_pending_plate(&mut self, who: &AccountId, round: u32) {
        if self.plates.get(who).is_some_and(|p| p.round == round && p.pending) {
            self.plates.remove(who);
        }
    }

    /// Counts a validator list against the protocol's own numbers.
    fn count(validators: &[AccountId]) -> Roll {
        let total = env::validator_total_stake().as_yoctonear();
        let mut sum = 0u128;
        let mut nakamoto = 0u32;
        let mut halting = 0u128;
        let mut squares = 0u128;
        let mut previous: Option<(u128, &AccountId)> = None;
        for (i, validator) in validators.iter().enumerate() {
            let stake = env::validator_stake(validator).as_yoctonear();
            require!(stake > 0, format!("RUNT[ROLL] {validator} is not an active validator this epoch"));
            if let Some((last_stake, last_id)) = previous {
                require!(
                    stake < last_stake || (stake == last_stake && validator.as_str() > last_id.as_str()),
                    format!("RUNT[ROLL] {validator} is out of order")
                );
            }
            sum += stake;
            if nakamoto == 0 && U256::from(sum) * U256::from(3u8) > U256::from(total) {
                nakamoto = (i + 1) as u32;
                halting = sum;
            }
            let share = mul_div(stake, MILLION, total);
            squares += share * share;
            previous = Some((stake, validator));
        }
        require!(sum == total, "RUNT[ROLL] the list is incomplete: its stake does not match the protocol's total");
        let effective = if squares == 0 { 0 } else { (100 * TRILLION / squares) as u64 };
        Roll {
            nakamoto,
            validators: validators.len() as u32,
            effective_x100: U64(effective),
            halting_share_ppm: mul_div(halting, MILLION, total) as u32,
        }
    }

    /// Closes every round whose time is up, in one call. Feeds only land in the open round, so only
    /// that one can hold anything; every round after it is known to be empty and is settled in a batch.
    fn close_rounds(&mut self) -> u32 {
        let now = self.clock_round();
        let first = self.open.round;
        if first >= now {
            return 0;
        }
        let closed = now - first;
        let result = self.close_one();
        let (kennel_in, goliath, hits) = (result.kennel_in, result.goliath, u32::from(result.hit));
        let spoiled = result.spoiled + self.close_idle_rounds(now);
        let here = env::current_account_id();
        let last = now - 1;
        if kennel_in > 0 {
            FtMint { owner_id: &here, amount: U128(kennel_in), memo: Some("kennel") }.emit();
            self.push_log(LogKind::Kennel, first, here.clone(), kennel_in);
        }
        if spoiled > 0 {
            FtBurn { owner_id: &here, amount: U128(spoiled), memo: Some("spoiled") }.emit();
            self.push_log(LogKind::Spoil, last, here.clone(), spoiled);
        }
        if goliath > 0 {
            FtBurn { owner_id: &here, amount: U128(goliath), memo: Some("goliath") }.emit();
            self.push_log(LogKind::Goliath, first, here, goliath);
        }
        event("rounds_closed", json!({
            "first": first, "last": last, "hits": hits, "misses": closed - hits,
            "kennel_in": U128(kennel_in), "burned": U128(spoiled + goliath),
            "streak": self.streak, "target": self.target,
        }));
        closed
    }

    fn close_one(&mut self) -> Closed {
        let here = env::current_account_id();
        let open = self.open.clone();
        let target_then = self.target;
        let rose = self.nakamoto_rose();
        let fell = self.nakamoto_fell();
        let hit = open.plates >= self.target;
        let (mut kennel_in, mut pot, mut spoiled, mut goliath) = (0u128, 0u128, 0u128, 0u128);

        if hit {
            let meal = self.meal_for(self.streak, rose);
            self.streak += 1;
            self.best_streak = self.best_streak.max(self.streak);
            kennel_in = mul_div(meal, u128::from(self.rules.kennel_bps), BPS);
            pot = meal - kennel_in;
            if kennel_in > 0 {
                self.token.internal_deposit(&here, kennel_in);
                self.minted += kennel_in;
            }
            self.meals.insert(open.round, Meal { pot: U128(pot), bites: open.bites, claimed: U128(0) });
            let up = u64::from(self.target) * (10_000 + u64::from(self.rules.target_up_bps));
            self.target = u32::try_from(up.div_ceil(10_000)).unwrap_or(u32::MAX);
        } else {
            self.streak = 0;
            spoiled = mul_div(self.kennel(), u128::from(self.rules.spoil_bps), BPS);
            self.burn_kennel(spoiled);
            self.target = self.target_after_miss(self.target);
        }
        if fell {
            goliath = mul_div(self.kennel(), u128::from(self.rules.nakamoto_down_burn_bps), BPS);
            self.burn_kennel(goliath);
        }
        if open.nakamoto > 0 {
            self.last_nakamoto = open.nakamoto;
            self.best_nakamoto = self.best_nakamoto.max(open.nakamoto);
        }
        self.push_round_line(RoundLine {
            round: open.round,
            through: open.round,
            hit,
            plates: open.plates,
            target: target_then,
            feeders: open.feeders,
            bites: open.bites,
            pot: U128(pot),
            kennel_in: U128(kennel_in),
            burned: U128(spoiled + goliath),
            streak: self.streak,
            nakamoto: open.nakamoto,
        });
        self.open = OpenRound { round: open.round + 1, ..OpenRound::default() };
        Closed { hit, kennel_in, spoiled, goliath }
    }

    /// Everything between the round just closed and now: nobody could have fed these. Each one still
    /// counts as a miss, so the Kennel spoils and the target falls just as if they had closed one by one.
    fn close_idle_rounds(&mut self, now: u32) -> u128 {
        let first = self.open.round;
        if first >= now {
            return 0;
        }
        let steps = (now - first).min(MAX_IDLE_STEPS);
        let spoil = u128::from(self.rules.spoil_bps);
        let mut kennel = self.kennel();
        let mut burned = 0u128;
        let mut target = self.target;
        for _ in 0..steps {
            let rot = mul_div(kennel, spoil, BPS);
            kennel -= rot;
            burned += rot;
            target = self.target_after_miss(target);
            if rot == 0 && target == self.rules.target_floor {
                break;
            }
        }
        self.burn_kennel(burned);
        self.target = target;
        self.streak = 0;
        self.push_round_line(RoundLine {
            round: first,
            through: now - 1,
            hit: false,
            plates: 0,
            target,
            feeders: 0,
            bites: U128(0),
            pot: U128(0),
            kennel_in: U128(0),
            burned: U128(burned),
            streak: 0,
            nakamoto: 0,
        });
        self.open = OpenRound { round: now, ..OpenRound::default() };
        burned
    }

    fn target_after_miss(&self, target: u32) -> u32 {
        let down = u64::from(target) * (10_000 - u64::from(self.rules.target_down_bps)) / 10_000;
        (down as u32).max(self.rules.target_floor)
    }

    fn burn_kennel(&mut self, amount: u128) {
        if amount > 0 {
            self.token.internal_withdraw(&env::current_account_id(), amount);
            self.burned += amount;
        }
    }

    /// Consecutive rounds that nobody fed become one line, so an idle coin doesn't grow its state.
    fn push_round_line(&mut self, line: RoundLine) {
        if line.plates == 0 && line.nakamoto == 0 && self.rounds.len() > 0 {
            let last = self.rounds.len() - 1;
            if let Some(previous) = self.rounds.get_mut(last) {
                if previous.plates == 0 && previous.nakamoto == 0 && previous.through + 1 == line.round {
                    previous.through = line.through;
                    previous.burned = U128(previous.burned.0 + line.burned.0);
                    previous.target = line.target;
                    return;
                }
            }
        }
        self.rounds.push(line);
    }

    fn push_log(&mut self, kind: LogKind, round: u32, account: AccountId, amount: u128) {
        let line = LogLine { kind, round, account, amount: U128(amount), at: U64(env::block_timestamp()) };
        if self.log_total < LOG_SIZE {
            self.log.push(line);
        } else {
            self.log.replace((self.log_total % LOG_SIZE) as u32, line);
        }
        self.log_total += 1;
    }

    /// Pays out a plate from a closed round, then clears it. Plates from missed rounds get nothing.
    fn pay_plate(&mut self, who: &AccountId) -> u128 {
        let Some(plate) = self.plates.get(who).cloned() else { return 0 };
        if plate.round >= self.open.round {
            return 0;
        }
        self.plates.remove(who);
        if plate.pending || plate.bite.0 == 0 {
            return 0;
        }
        let Some(meal) = self.meals.get_mut(&plate.round) else { return 0 };
        let reward = mul_div(meal.pot.0, plate.bite.0, meal.bites.0);
        meal.claimed = U128(meal.claimed.0 + reward);
        if reward == 0 {
            return 0;
        }
        self.token.internal_deposit(who, reward);
        self.minted += reward;
        FtMint { owner_id: who, amount: U128(reward), memo: Some("meal") }.emit();
        self.push_log(LogKind::Meal, plate.round, who.clone(), reward);
        reward
    }
}

struct Closed {
    hit: bool,
    kennel_in: u128,
    spoiled: u128,
    goliath: u128,
}

// ---- NEP-141 ----------------------------------------------------------------------------------

#[near]
impl FungibleTokenCore for Runt {
    #[payable]
    fn ft_transfer(&mut self, receiver_id: AccountId, amount: U128, memo: Option<String>) {
        self.token.ft_transfer(receiver_id, amount, memo)
    }

    #[payable]
    fn ft_transfer_call(&mut self, receiver_id: AccountId, amount: U128, memo: Option<String>, msg: String) -> PromiseOrValue<U128> {
        // The Kennel is this contract's own balance. Tokens parked here for a moment and refunded
        // would inflate a burn taken in between, so a holder could burn the Kennel for free.
        require!(receiver_id != env::current_account_id(), "RUNT[KENNEL] ft_transfer_call can't target the coin itself");
        self.token.ft_transfer_call(receiver_id, amount, memo, msg)
    }

    fn ft_total_supply(&self) -> U128 {
        self.token.ft_total_supply()
    }

    fn ft_balance_of(&self, account_id: AccountId) -> U128 {
        self.token.ft_balance_of(account_id)
    }
}

#[near]
impl FungibleTokenResolver for Runt {
    #[private]
    fn ft_resolve_transfer(&mut self, sender_id: AccountId, receiver_id: AccountId, amount: U128) -> U128 {
        let (used, burned) = self.token.internal_ft_resolve_transfer(&sender_id, receiver_id, amount);
        if burned > 0 {
            // The sender left mid-transfer. The standard burns what could not be returned, and
            // emits the ft_burn event itself.
            self.burned += burned;
            self.push_log(LogKind::Unregister, self.open.round, sender_id, burned);
        }
        used.into()
    }
}

// ---- NEP-145 ----------------------------------------------------------------------------------

#[near]
impl StorageManagement for Runt {
    #[payable]
    fn storage_deposit(&mut self, account_id: Option<AccountId>, registration_only: Option<bool>) -> StorageBalance {
        self.token.storage_deposit(account_id, registration_only)
    }

    #[payable]
    fn storage_withdraw(&mut self, amount: Option<NearToken>) -> StorageBalance {
        self.token.storage_withdraw(amount)
    }

    /// Any meal already earned is paid first. An account that has fed the open round can't leave
    /// until that round closes: leaving and rejoining would otherwise allow a second feed, and dilute
    /// everyone else's share. With `force`, the balance is burned and logged.
    #[payable]
    fn storage_unregister(&mut self, force: Option<bool>) -> bool {
        let who = env::predecessor_account_id();
        if self.token.accounts.contains_key(&who) {
            self.close_rounds();
            self.pay_plate(&who);
            require!(!self.plates.contains_key(&who), "RUNT[FED] you can leave once the round you fed has closed");
        }
        let Some((_, balance)) = self.token.internal_storage_unregister(force) else { return false };
        if balance > 0 {
            self.burned += balance;
            FtBurn { owner_id: &who, amount: U128(balance), memo: Some("unregister") }.emit();
            self.push_log(LogKind::Unregister, self.open.round, who, balance);
        }
        true
    }

    fn storage_balance_bounds(&self) -> StorageBalanceBounds {
        self.token.storage_balance_bounds()
    }

    fn storage_balance_of(&self, account_id: AccountId) -> Option<StorageBalance> {
        self.token.storage_balance_of(account_id)
    }
}

// ---- NEP-148 ----------------------------------------------------------------------------------

#[near]
impl FungibleTokenMetadataProvider for Runt {
    fn ft_metadata(&self) -> FungibleTokenMetadata {
        FungibleTokenMetadata {
            spec: FT_METADATA_SPEC.to_string(),
            name: "Runt".to_string(),
            symbol: "RUNT".to_string(),
            icon: Some(ICON.to_string()),
            reference: None,
            reference_hash: None,
            decimals: 18,
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
