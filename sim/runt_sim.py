#!/usr/bin/env python3
"""RUNT: 60 days of community behaviour on NEAR's real validator set.

Every scenario starts from the mainnet validator snapshot in data/. The
protocol numbers are real. The people are invented, so this is a stress test
of the rules, not a forecast of what anyone will do.

    python sim/runt_sim.py baseline [runs]   outcome table and supply by day
    python sim/runt_sim.py supply            print supply by day from saved results
    python sim/runt_sim.py sweep [runs]      try every parameter set
    python sim/runt_sim.py picks             explain each launch value from the sweep
    python sim/runt_sim.py vectors           write the numbers the Rust tests check
    python sim/runt_sim.py export            write the data the token page charts
    python sim/runt_sim.py tables            print the README's tables from saved results

Standard library only.
"""
from __future__ import annotations

import itertools
import json
import math
import os
import random
import statistics
import sys
from dataclasses import asdict, dataclass, field, replace

HERE = os.path.dirname(os.path.abspath(__file__))
RESULTS = os.path.join(HERE, "results")
SNAPSHOT = json.load(open(os.path.join(HERE, "data", "mainnet_validators.json"), encoding="utf8"))
NAMES = [v[0] for v in SNAPSHOT["validators"]]
BASE_STAKE = [float(v[1]) for v in SNAPSHOT["validators"]]
# Only pools from the locked factory can be fed. The older pool.near factory still has access keys
# and its pools run more than one version of the code, so the mainnet rules leave it out.
IS_POOL = [n.endswith(".poolv1.near") for n in NAMES]

DAYS = 60


@dataclass(frozen=True)
class Rules:
    """The launch parameters. In the contract these are set once, in `new`."""

    size_cap_bps: int = 100        # validators at this share of stake or above have no hunger
    meal: float = 1_000_000.0      # RUNT per hit round before the streak bonus
    streak_step_bps: int = 200     # extra meal per round of streak
    streak_cap: int = 30           # the streak bonus stops growing here
    kennel_bps: int = 2000         # share of each meal kept in the Kennel
    spoil_bps: int = 2500          # share of the Kennel burned on a miss
    target_start: int = 5          # different runts to feed in the first round
    target_floor: int = 3          # the target never falls below this
    target_up_bps: int = 1000      # target growth after a hit
    target_down_bps: int = 3000    # target fall after a miss
    min_stake: float = 10.0        # the least stake that counts, in NEAR
    full_bite: float = 1_000.0     # NEAR per runt per round that counts in full; a square root above it
    census_bounty: float = 5_000.0 # paid from the Kennel for the round's roll call
    nc_up_bonus_bps: int = 20000   # extra meal in a round where the Nakamoto number rose
    nc_down_burn_bps: int = 5000   # share of the Kennel burned when it fell


LAUNCH = Rules()


def hunger(share: float, r: Rules) -> float:
    """1.0 for a validator with no stake, falling to 0 at the size cap."""
    cap = r.size_cap_bps / 10_000
    if share >= cap:
        return 0.0
    return ((cap - share) / cap) ** 2


def curve(fed: float, full: float) -> float:
    """Linear up to `full` NEAR, then a square root. The two meet at `full`."""
    return fed / math.sqrt(full) if fed <= full else math.sqrt(fed)


def bite(h: float, fed_before: float, stake: float, full: float) -> float:
    """A feeder's points: how far they move the runt up the curve this round.

    Splitting between accounts on one runt changes nothing, because the steps add
    up to the same climb. Below `full` points are proportional to NEAR, so dust
    earns dust. Above it each extra NEAR on the same runt earns less, so the way
    to earn more is to spread real stake over more validators.
    """
    return h * (curve(fed_before + stake, full) - curve(fed_before, full))


def nakamoto(stakes: list[float]) -> int:
    """The fewest validators that together hold more than a third of all stake."""
    ordered = sorted(stakes, reverse=True)
    third = sum(ordered) / 3
    total = 0.0
    for i, s in enumerate(ordered):
        total += s
        if total > third:
            return i + 1
    return len(ordered)


def top_share(stakes: list[float], n: int = 9) -> float:
    return sum(sorted(stakes, reverse=True)[:n]) / sum(stakes)


def effective_validators(stakes: list[float]) -> float:
    total = sum(stakes)
    return 1 / sum((s / total) ** 2 for s in stakes)


# ----------------------------------------------------------------------------- people


@dataclass
class Dog:
    stake: float
    pool: int = -1          # validator this dog's NEAR is delegated to
    home: int = -1          # where the NEAR sat before, or -1 for fresh NEAR
    ready_day: int = 0      # unstaking takes four epochs, so a move costs two rounds
    feed_p: float = 0.75
    leave_p: float = 0.01
    gone: bool = False
    earned: float = 0.0
    tag: str = ""


@dataclass
class Scenario:
    label: str
    joins: list = field(default_factory=list)      # joiners per day
    feed_p: float = 0.75
    leave_p: float = 0.01
    policy: str = "best"                           # how a dog picks a runt
    median_stake: float = 300.0
    spread: float = 1.4
    moved_share: float = 0.7                       # share of joiners who move NEAR off another validator
    special: str = ""


def steady_joins(total: int, midpoint: float, rate: float) -> list[int]:
    curve = [total / (1 + math.exp(-rate * (d - midpoint))) for d in range(DAYS + 1)]
    return [max(0, round(curve[d + 1] - curve[d])) for d in range(DAYS)]


def burst_joins(total: int, peak: int, width: float) -> list[int]:
    weights = [math.exp(-(((d - peak) / width) ** 2)) for d in range(DAYS)]
    scale = total / sum(weights)
    return [round(w * scale) for w in weights]


def few_joins(total: int, days: int) -> list[int]:
    out = [0] * DAYS
    for d in range(days):
        out[d] = total // days
    return out


STEADY = steady_joins(400, 16, 0.25)
SCENARIOS = {
    "steady": Scenario("Steady pack", STEADY),
    "hype": Scenario("Hype then fade", burst_joins(1500, 8, 5.0), leave_p=0.06, median_stake=150.0),
    "whale": Scenario("Whale moves in", STEADY, special="whale"),
    "sybil": Scenario("Sybil swarm", STEADY, special="sybil"),
    "herd": Scenario("Herd piles on one runt", STEADY, policy="hungriest"),
    "ghost": Scenario("Ghost town", few_joins(30, 6), feed_p=0.5, leave_p=0.12),
    "small": Scenario("Small but loyal", few_joins(40, 8), feed_p=0.85, leave_p=0.005),
    "merger": Scenario("Two top pools merge", STEADY, special="merger"),
    "spreader": Scenario("Whale spreads out", STEADY, special="spreader"),
    "exodus": Scenario("Delegators really move", steady_joins(900, 22, 0.2), special="exodus", median_stake=400.0),
}


def pick_runt(rng: random.Random, dog: Dog, policy: str, shares: list[float], eligible: list[int], fed: dict, r: Rules) -> int:
    if policy == "hungriest":
        return min(eligible, key=lambda i: shares[i])
    if policy == "random":
        return rng.choice(eligible)
    # Nobody reads 300 rows. A dog compares a page's worth of options, then takes the best bite.
    options = rng.sample(eligible, min(30, len(eligible)))
    return max(options, key=lambda i: bite(hunger(shares[i], r), fed.get(i, 0.0), dog.stake, r.full_bite))


def simulate(r: Rules, s: Scenario, seed: int, days: int = DAYS) -> dict:
    rng = random.Random(seed)
    stakes = list(BASE_STAKE)
    dogs: list[Dog] = []
    fed_yesterday: dict[int, float] = {}
    pending: list[tuple[int, int, float]] = []   # stake arriving on a later day

    target = r.target_start
    streak = best_streak = 0
    kennel = minted = burned = 0.0
    last_nc = nakamoto(stakes)
    start_nc = last_nc
    start_top = top_share(stakes)
    best_nc = last_nc
    rows = []
    earned_control = 0.0
    hits = 0

    def move(dog: Dog, to: int, day: int) -> None:
        """Unstaking takes four epochs, about a day and a half. Fresh NEAR can stake straight away."""
        delay = 0
        if dog.pool >= 0:
            stakes[dog.pool] -= dog.stake
            delay = 2
        elif dog.home >= 0:
            stakes[dog.home] -= dog.stake
            delay = 2
        pending.append((day + delay, to, dog.stake))
        dog.pool = to
        dog.ready_day = day + delay

    for day in range(days):
        total = sum(stakes)
        shares = [x / total for x in stakes]
        eligible = [i for i in range(len(stakes)) if IS_POOL[i] and stakes[i] > 0 and shares[i] < r.size_cap_bps / 10_000]

        # New arrivals, weighted towards where delegated NEAR already sits.
        for _ in range(s.joins[day]):
            stake = max(r.min_stake, rng.lognormvariate(math.log(s.median_stake), s.spread))
            dog = Dog(stake=stake, feed_p=s.feed_p, leave_p=s.leave_p)
            if rng.random() < s.moved_share:
                dog.home = rng.choices(range(len(stakes)), weights=stakes, k=1)[0]
            target_pool = pick_runt(rng, dog, s.policy, shares, eligible, fed_yesterday, r)
            move(dog, target_pool, day)
            dogs.append(dog)

        # Scripted events.
        if s.special == "whale" and day == 15:
            big = Dog(stake=2_000_000.0, home=0, feed_p=1.0, leave_p=0.0, tag="whale")
            move(big, pick_runt(rng, big, "best", shares, eligible, fed_yesterday, r), day)
            dogs.append(big)
        if s.special == "sybil" and day == 10:
            # One person with 2,000 NEAR splits it over 20 accounts and 20 runts.
            spots = rng.sample(eligible, 20)
            for spot in spots:
                dog = Dog(stake=100.0, feed_p=1.0, leave_p=0.0, tag="sybil")
                move(dog, spot, day)
                dogs.append(dog)
            # Another splits 2,000 NEAR as thinly as the rules allow.
            spots = rng.sample(eligible, min(200, len(eligible)))
            for spot in spots:
                dog = Dog(stake=2_000.0 / len(spots), feed_p=1.0, leave_p=0.0, tag="dust")
                move(dog, spot, day)
                dogs.append(dog)
            # An honest control with the same NEAR in one account.
            control = Dog(stake=2_000.0, feed_p=1.0, leave_p=0.0, tag="control")
            move(control, pick_runt(rng, control, "best", shares, eligible, fed_yesterday, r), day)
            dogs.append(control)
        if s.special == "merger" and day == 25:
            stakes[1] += stakes[2]     # ranks 2 and 3 become one pool
            stakes[2] = 0.0
        if s.special == "spreader" and day == 15:
            # The same whale, but split over 40 runts.
            for spot in rng.sample(eligible, 40):
                dog = Dog(stake=50_000.0, home=0, feed_p=1.0, leave_p=0.0, tag="whale")
                move(dog, spot, day)
                dogs.append(dog)
        if s.special == "exodus" and 8 <= day <= 40 and day % 4 == 0:
            # Large delegators leave the top validators and spread out.
            source = rng.randrange(0, 9)
            chunk = stakes[source] * 0.02
            for spot in rng.sample(eligible, 8):
                dog = Dog(stake=chunk / 8, home=source, feed_p=0.9, leave_p=0.0, tag="mover")
                move(dog, spot, day)
                dogs.append(dog)

        # Stake that finished unbonding lands now, and counts for shares from tomorrow.
        for item in [p for p in pending if p[0] <= day]:
            stakes[item[1]] += item[2]
            pending.remove(item)

        # Mealtime: everyone who shows up, in a random order.
        feeders = [d for d in dogs if not d.gone and d.ready_day <= day and d.pool >= 0]
        rng.shuffle(feeders)
        fed_today: dict[int, float] = {}
        bites: list[tuple[Dog, float]] = []
        total_bites = 0.0
        for d in feeders:
            if rng.random() > d.feed_p or d.stake < r.min_stake:
                continue
            h = hunger(shares[d.pool], r)
            if h <= 0:
                continue
            b = bite(h, fed_today.get(d.pool, 0.0), d.stake, r.full_bite)
            fed_today[d.pool] = fed_today.get(d.pool, 0.0) + d.stake
            bites.append((d, b))
            total_bites += b
        plates = len(fed_today)

        # Close the round, using the same order as the contract.
        new_nc = nakamoto(stakes)
        hit = plates >= target and plates > 0
        meal_paid = 0.0
        burn = 0.0
        if hit:
            hits += 1
            bonus_steps = min(streak, r.streak_cap)
            streak += 1
            best_streak = max(best_streak, streak)
            meal = r.meal * (10_000 + bonus_steps * r.streak_step_bps) / 10_000
            if new_nc > last_nc:
                meal *= (10_000 + r.nc_up_bonus_bps) / 10_000
            kennel_cut = meal * r.kennel_bps / 10_000
            pot = meal - kennel_cut
            kennel += kennel_cut
            minted += meal
            meal_paid = meal
            for d, b in bites:
                d.earned += pot * b / total_bites
            target = max(r.target_floor, math.ceil(target * (10_000 + r.target_up_bps) / 10_000))
        else:
            streak = 0
            burn = kennel * r.spoil_bps / 10_000
            kennel -= burn
            burned += burn
            target = max(r.target_floor, math.floor(target * (10_000 - r.target_down_bps) / 10_000))
        if new_nc < last_nc:
            extra = kennel * r.nc_down_burn_bps / 10_000
            kennel -= extra
            burned += extra
            burn += extra
        pay = min(kennel, r.census_bounty)
        kennel -= pay
        best_nc = max(best_nc, new_nc)
        last_nc = new_nc

        rows.append({
            "day": day + 1, "plates": plates, "target_next": target, "hit": hit, "streak": streak,
            "supply": minted - burned, "kennel": kennel, "burned": burned, "meal": meal_paid,
            "nc": new_nc, "top9": top_share(stakes),
        })
        fed_yesterday = fed_today

        # Some dogs lose interest. A third of those take their NEAR back to a big validator.
        for d in dogs:
            if not d.gone and rng.random() < d.leave_p:
                d.gone = True
                if rng.random() < 0.33 and d.pool >= 0:
                    stakes[d.pool] -= d.stake
                    back = rng.choices(range(9), weights=stakes[:9], k=1)[0]
                    stakes[back] += d.stake

    # Moved stake: NEAR sitting on eligible validators because of RUNT.
    moved = sum(d.stake for d in dogs if d.pool >= 0 and not d.gone)
    biggest_cut = max((d.earned for d in dogs), default=0.0)
    all_earned = sum(d.earned for d in dogs) or 1.0
    result = {
        "rows": rows, "hit_rate": hits / days, "best_streak": best_streak, "supply": minted - burned,
        "burned": burned, "kennel": kennel, "start_nc": start_nc, "final_nc": last_nc, "best_nc": best_nc,
        "top9_start": start_top, "top9_end": top_share(stakes), "moved_near": moved,
        "effective_end": effective_validators(stakes), "max_share_of_rewards": biggest_cut / all_earned,
    }
    tagged = lambda tag: sum(d.earned for d in dogs if d.tag == tag)
    if s.special == "sybil":
        result["sybil_earned"] = tagged("sybil")
        result["dust_earned"] = tagged("dust")
        result["control_earned"] = tagged("control")
    if s.special in ("whale", "spreader"):
        result["whale_share"] = tagged("whale") / all_earned
    return result


# ----------------------------------------------------------------------------- reports


def median(values):
    return statistics.median(values)


def run_many(r: Rules, s: Scenario, runs: int, base_seed: int = 1000) -> list[dict]:
    return [simulate(r, s, base_seed + i) for i in range(runs)]


def summary(results: list[dict]) -> dict:
    keys = ["hit_rate", "best_streak", "supply", "burned", "kennel", "final_nc", "best_nc", "top9_end", "moved_near", "max_share_of_rewards", "effective_end"]
    out = {k: median([x[k] for x in results]) for k in keys}
    out["start_nc"] = results[0]["start_nc"]
    out["top9_start"] = results[0]["top9_start"]
    for extra in ("sybil_earned", "dust_earned", "control_earned", "whale_share"):
        if extra in results[0]:
            out[extra] = median([x[extra] for x in results])
    return out


SHOW_DAYS = [1, 5, 10, 20, 30, 40, 50, 60]


def supply_table(data: dict) -> None:
    print("\nSupply over time, millions of RUNT (median of runs):\n")
    print(f"{'Scenario':26s}" + "".join(f"{'Day ' + str(d):>10s}" for d in SHOW_DAYS))
    for key, item in data.items():
        cells = "".join(f"{item['supply_by_day'][d - 1] / 1e6:>10.1f}" for d in SHOW_DAYS)
        print(f"{item['label']:26s}{cells}")


def baseline(runs: int) -> None:
    os.makedirs(RESULTS, exist_ok=True)
    data = {}
    for key, s in SCENARIOS.items():
        results = run_many(LAUNCH, s, runs)
        by_day = lambda field_name: [median([x["rows"][d][field_name] for x in results]) for d in range(DAYS)]
        data[key] = {
            "label": s.label,
            "summary": summary(results),
            "supply_by_day": by_day("supply"),
            "kennel_by_day": by_day("kennel"),
            "burned_by_day": by_day("burned"),
            "plates_by_day": by_day("plates"),
            "target_by_day": by_day("target_next"),
            "nc_by_day": by_day("nc"),
            "top9_by_day": by_day("top9"),
        }
    with open(os.path.join(RESULTS, "baseline.json"), "w", encoding="utf8") as f:
        json.dump({"runs": runs, "rules": asdict(LAUNCH), "scenarios": data}, f, indent=1)

    print(f"\nMedians of {runs} runs on the launch parameters. Real validator set, invented people.\n")
    head = f"{'Scenario':26s}{'Hit rate':>9s}{'Streak':>8s}{'Supply M':>10s}{'Burned M':>10s}{'NC':>9s}{'Top 9':>16s}{'Moved NEAR':>13s}{'Top earner':>11s}"
    print(head)
    for key, item in data.items():
        m = item["summary"]
        print(f"{item['label']:26s}{m['hit_rate'] * 100:>8.0f}%{m['best_streak']:>8.0f}{m['supply'] / 1e6:>10.1f}{m['burned'] / 1e6:>10.2f}"
              f"{str(m['start_nc']) + ' to ' + str(int(m['final_nc'])):>9s}{m['top9_start'] * 100:>9.2f}% {m['top9_end'] * 100:>5.2f}%{m['moved_near']:>13,.0f}{m['max_share_of_rewards'] * 100:>10.1f}%")
    sybil = data["sybil"]["summary"]
    print(f"\nSybil check, 2,000 NEAR each: one account {sybil['control_earned']:,.0f} RUNT; 20 accounts on 20 runts {sybil['sybil_earned']:,.0f}; as thin as the rules allow {sybil['dust_earned']:,.0f}.")
    print(f"Whale check: 2M NEAR in one account took {data['whale']['summary']['whale_share'] * 100:.1f}% of all rewards. Spread over 40 runts it took {data['spreader']['summary']['whale_share'] * 100:.1f}%.")
    supply_table(data)


def supply_from_saved() -> None:
    data = json.load(open(os.path.join(RESULTS, "baseline.json"), encoding="utf8"))
    supply_table(data["scenarios"])


# The sweep tries every combination, on the scenarios that pull the rules in different directions.
GRID = {
    "size_cap_bps": [50, 100, 200],
    "full_bite": [100.0, 1000.0, 10000.0],
    "target_up_bps": [500, 1000, 1500],
    "target_down_bps": [2000, 3000, 4000],
    "kennel_bps": [1000, 2000, 3000],
    "spoil_bps": [1000, 2500, 5000],
}
SWEEP_SCENARIOS = ["steady", "hype", "whale", "ghost"]


def sweep(runs: int) -> None:
    os.makedirs(RESULTS, exist_ok=True)
    keys = list(GRID)
    rows = []
    combos = list(itertools.product(*GRID.values()))
    for n, values in enumerate(combos):
        rules = replace(LAUNCH, **dict(zip(keys, values)))
        row = {"params": dict(zip(keys, values))}
        for name in SWEEP_SCENARIOS:
            results = run_many(rules, SCENARIOS[name], runs, base_seed=5000)
            row[name] = summary(results)
        rows.append(row)
        if (n + 1) % 27 == 0:
            print(f"{n + 1}/{len(combos)} parameter sets", flush=True)
    with open(os.path.join(RESULTS, "sweep.json"), "w", encoding="utf8") as f:
        json.dump({"runs": runs, "grid": GRID, "scenarios": SWEEP_SCENARIOS, "rows": rows}, f, indent=0)
    print(f"Saved {len(rows)} parameter sets, {runs} runs each, on {len(SWEEP_SCENARIOS)} scenarios.")


def picks() -> None:
    data = json.load(open(os.path.join(RESULTS, "sweep.json"), encoding="utf8"))
    rows = data["rows"]
    print(f"\n{len(rows)} parameter sets, {data['runs']} runs each. Each figure is the median over every set using that value.\n")
    def med(rows_for_value, scenario, metric):
        return median([r[scenario][metric] for r in rows_for_value])
    for param, values in data["grid"].items():
        launch_value = getattr(LAUNCH, param)
        print(f"{param} (launch: {launch_value})")
        print(f"  {'value':>7s}{'steady hit':>12s}{'steady streak':>15s}{'hype hit':>10s}{'ghost supply M':>16s}{'steady supply M':>17s}{'steady burned M':>17s}{'whale share':>13s}")
        for value in values:
            chosen = [r for r in rows if r["params"][param] == value]
            print(f"  {value:>7.0f}{med(chosen, 'steady', 'hit_rate') * 100:>11.0f}%{med(chosen, 'steady', 'best_streak'):>15.0f}{med(chosen, 'hype', 'hit_rate') * 100:>9.0f}%"
                  f"{med(chosen, 'ghost', 'supply') / 1e6:>16.2f}{med(chosen, 'steady', 'supply') / 1e6:>17.1f}{med(chosen, 'steady', 'burned') / 1e6:>17.2f}{med(chosen, 'whale', 'whale_share') * 100:>12.1f}%")
        print()


def export() -> None:
    """Writes the small file the token page charts from."""
    data = json.load(open(os.path.join(RESULTS, "baseline.json"), encoding="utf8"))
    out = {
        "runs": data["runs"],
        "scenarios": {},
        "mainnet": {"fetched": SNAPSHOT["fetched"], "validators": SNAPSHOT["validators"]},
    }
    for key, item in data["scenarios"].items():
        out["scenarios"][key] = {
            "label": item["label"],
            "supply": [round(v / 1e6, 2) for v in item["supply_by_day"]],
            "summary": {k: item["summary"][k] for k in ("hit_rate", "best_streak", "supply", "burned", "final_nc", "moved_near")},
        }
    target = os.path.join(HERE, "..", "web", "src", "sim-data.json")
    with open(target, "w", encoding="utf8") as f:
        json.dump(out, f, separators=(",", ":"))
    print("Wrote web/src/sim-data.json")


def tables() -> None:
    """Prints the README's tables from the saved results, so the two can't disagree."""
    data = json.load(open(os.path.join(RESULTS, "baseline.json"), encoding="utf8"))
    print(f"Medians of {data['runs']} runs on the launch parameters:\n")
    print("| Scenario | Hit rate | Best streak | Supply, M | Burned, M | Number, day 60 | Top 9 share | NEAR moved | Top earner |")
    print("|---|---|---|---|---|---|---|---|---|")
    for item in data["scenarios"].values():
        m = item["summary"]
        print(f"| {item['label']} | {m['hit_rate'] * 100:.0f}% | {m['best_streak']:.0f} | {m['supply'] / 1e6:.1f} | {m['burned'] / 1e6:.1f} | {m['start_nc']} to {int(m['final_nc'])} | {m['top9_end'] * 100:.2f}% | {m['moved_near']:,.0f} | {m['max_share_of_rewards'] * 100:.1f}% |")
    print(f"\nSupply over time, in millions of RUNT, median of {data['runs']} runs:\n")
    print("| Scenario | " + " | ".join(f"Day {d}" for d in SHOW_DAYS) + " |")
    print("|---|" + "---|" * len(SHOW_DAYS))
    for item in data["scenarios"].values():
        print(f"| {item['label']} | " + " | ".join(f"{item['supply_by_day'][d - 1] / 1e6:.1f}" for d in SHOW_DAYS) + " |")
    sybil = data["scenarios"]["sybil"]["summary"]
    print(f"\nSybil: one account {sybil['control_earned']:,.0f}, 20 accounts {sybil['sybil_earned']:,.0f}, thinnest {sybil['dust_earned']:,.0f}.")
    print(f"Whale: one account {data['scenarios']['whale']['summary']['whale_share'] * 100:.1f}%, spread {data['scenarios']['spreader']['summary']['whale_share'] * 100:.1f}%.")


def vectors() -> None:
    """Numbers the Rust tests check, so the simulator and the contract cannot drift apart."""
    r = LAUNCH
    cases = []
    for share_bps in (0, 1, 10, 25, 50, 75, 99, 100, 150):
        h = hunger(share_bps / 10_000, r)
        cases.append({"share_bps": share_bps, "hunger_bps": int(h * 10_000 + 1e-9)})
    bites = []
    for hunger_bps, before, stake in ((10_000, 0, 100), (10_000, 100, 300), (5_000, 400, 500), (8_100, 0, 2_500), (10_000, 900, 200), (10_000, 2_000, 2_000)):
        value = bite(hunger_bps / 10_000, float(before), float(stake), r.full_bite)
        bites.append({"hunger_bps": hunger_bps, "before": before, "stake": stake, "bite_micro": round(value * 1_000_000)})
    with open(os.path.join(HERE, "vectors.json"), "w", encoding="utf8") as f:
        json.dump({"size_cap_bps": r.size_cap_bps, "full_bite": int(r.full_bite), "hunger": cases, "bites": bites}, f, indent=1)
    print("Wrote sim/vectors.json")


if __name__ == "__main__":
    command = sys.argv[1] if len(sys.argv) > 1 else "baseline"
    runs_arg = int(sys.argv[2]) if len(sys.argv) > 2 else None
    if command == "baseline":
        baseline(runs_arg or 30)
    elif command == "supply":
        supply_from_saved()
    elif command == "sweep":
        sweep(runs_arg or 4)
    elif command == "picks":
        picks()
    elif command == "vectors":
        vectors()
    elif command == "export":
        export()
    elif command == "tables":
        tables()
    else:
        print(__doc__)
