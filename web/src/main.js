import { COIN, EXPLORER, NETWORK, ONE_NEAR, ONE_RUNT } from "./config.js";
import { accessKeys, accountInfo, fromSnapshot, validators, view } from "./chain.js";
import { account, call, connect, disconnect, onAccount, restore, send } from "./wallet.js";
import { drawLitter, drawSupply } from "./charts.js";
import simData from "./sim-data.json";

const $ = (id) => document.getElementById(id);
const SPEEDRUN = "speedrun.runt-hq.testnet";

const state = { status: null, rules: null, testnet: null, mainnet: null, hungers: new Map(), fees: new Map(), net: "testnet", feedTarget: null };

// ---- Formatting -------------------------------------------------------------------------------

const commas = (n) => n.toLocaleString("en-US");

function runt(units) {
  const value = BigInt(units);
  const whole = value / ONE_RUNT;
  if (whole >= 1000n || value === 0n) return commas(whole);
  return (Number(value / 10n ** 14n) / 10_000).toLocaleString("en-US", { maximumFractionDigits: 2 });
}

function nearText(yocto, digits = 0) {
  const value = BigInt(yocto);
  if (digits === 0) return commas(value / ONE_NEAR);
  return (Number(value / 10n ** 20n) / 10_000).toLocaleString("en-US", { maximumFractionDigits: digits });
}

const percent = (share) => {
  const p = share * 100;
  if (p >= 10) return `${p.toFixed(1)}%`;
  if (p >= 0.1) return `${p.toFixed(2)}%`;
  return `${p.toFixed(3)}%`;
};

const bpsText = (bps) => `${(bps / 100).toLocaleString("en-US", { maximumFractionDigits: 2 })}%`;

function duration(seconds) {
  const s = Math.max(0, Math.floor(seconds));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const rest = String(s % 60).padStart(2, "0");
  return h ? `${h}:${String(m).padStart(2, "0")}:${rest}` : `${m}:${rest}`;
}

function humanSeconds(seconds) {
  if (seconds % 86400 === 0) return seconds === 86400 ? "1 day" : `${seconds / 86400} days`;
  if (seconds % 3600 === 0) return `${seconds / 3600} h`;
  return `${Math.round(seconds / 60)} min`;
}

function setText(id, value) {
  const element = $(id);
  if (element.textContent !== String(value)) element.textContent = value;
}

function make(tag, className, text) {
  const element = document.createElement(tag);
  if (className) element.className = className;
  if (text !== undefined) element.textContent = text;
  return element;
}

let toastTimer;
function toast(text, ms = 5000) {
  const box = $("toast");
  box.textContent = text;
  box.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (box.hidden = true), ms);
}

const friendly = (error) => {
  const message = String(error?.message ?? error);
  const code = message.match(/RUNT\[[A-Z]+\][^"\\]*/);
  return code ? code[0] : message.slice(0, 180);
};

// ---- The mechanics, mirrored for previews -------------------------------------------------------

function hungerOf(share, capBps) {
  const cap = capBps / 10_000;
  return share >= cap ? 0 : ((cap - share) / cap) ** 2;
}

function curve(fed, full) {
  return fed <= full ? fed / Math.sqrt(full) : Math.sqrt(fed);
}

function biteOf(hunger, fedBefore, stake, full) {
  return hunger * (curve(fedBefore + stake, full) - curve(fedBefore, full));
}

function mealSize() {
  const { rules, status } = state;
  const steps = Math.min(status.streak, rules.streak_cap);
  return (BigInt(rules.meal) * BigInt(10_000 + steps * rules.streak_step_bps)) / 10_000n;
}

const isPool = (id) => state.rules?.pool_suffixes.some((suffix) => id.endsWith(`.${suffix}`) && id.length > suffix.length + 1);

// ---- Reading the chain --------------------------------------------------------------------------

async function loadValidators() {
  const [testnet, mainnet] = await Promise.all([validators("testnet"), validators("mainnet").catch(() => null)]);
  state.testnet = testnet;
  if (mainnet) {
    state.mainnet = mainnet;
    state.mainnetLive = true;
  } else if (!state.mainnet) {
    state.mainnet = fromSnapshot(simData.mainnet.validators);
    state.mainnetLive = false;
  }
  renderHero();
}

async function loadFees() {
  if (!state.testnet || !state.rules) return;
  await Promise.all(
    state.testnet.list.filter((v) => isPool(v.id) && !state.fees.has(v.id)).map(async (v) => {
      try {
        const fee = await view(NETWORK, v.id, "get_reward_fee_fraction");
        state.fees.set(v.id, fee.numerator / fee.denominator);
      } catch {
        state.fees.set(v.id, null);
      }
    }),
  );
}

async function loadCoin() {
  if (!state.rules) state.rules = await view(NETWORK, COIN, "rules");
  state.status = await view(NETWORK, COIN, "status");
  if (state.testnet) {
    const pools = state.testnet.list.filter((v) => isPool(v.id)).map((v) => v.id);
    const rows = await view(NETWORK, COIN, "hungers", { pools });
    state.hungers = new Map(pools.map((id, i) => [id, rows[i]]));
  }
}

async function loadLogs() {
  const total = state.status.rounds_recorded;
  const stamp = `${total}:${state.status.log_total}:${state.status.open.round}`;
  if (stamp === state.logStamp) return;
  state.logStamp = stamp;
  const [events, rounds] = await Promise.all([
    view(NETWORK, COIN, "log", { limit: 128 }),
    view(NETWORK, COIN, "rounds", { from: Math.max(0, total - 200), limit: 200 }),
  ]);
  renderEvents(events);
  renderRounds(rounds.reverse());
}

async function loadMe() {
  const who = account();
  if (!who) return;
  const [plate, claimable, balance] = await Promise.all([
    view(NETWORK, COIN, "plate_of", { account_id: who }),
    view(NETWORK, COIN, "claimable", { account_id: who }),
    view(NETWORK, COIN, "ft_balance_of", { account_id: who }),
  ]);
  state.me = { plate, claimable: BigInt(claimable), balance: BigInt(balance) };
  renderMe();
}

// ---- Drawing -------------------------------------------------------------------------------------

function renderHero() {
  const main = state.mainnet;
  if (main) {
    setText("hero-where", state.mainnetLive ? "NEAR mainnet, live" : `NEAR mainnet, as of ${simData.mainnet.fetched}`);
    setText("hero-nc", main.nakamoto);
    setText("hero-text", `of ${main.list.length} validators hold more than a third of NEAR's stake.`);
    $("whatif").replaceChildren(
      document.createTextNode("Moving "),
      make("strong", "", `${nearText(main.toMove)} NEAR`),
      document.createTextNode(" off the biggest validators would make this number "),
      make("strong", "", String(main.nakamoto + 1)),
      document.createTextNode("."),
    );
  }
}

function renderRound() {
  const { status, rules } = state;
  if (!status) return;
  const over = status.clock_round > status.open.round;
  const round = status.open.round + 1;
  setText("round-label", `Round ${commas(round)}`);
  setText("plates", `${status.open.plates} of ${status.target}`);
  setText("feeders", commas(status.open.feeders));
  setText("streak", commas(status.streak));
  setText("best", commas(status.best_streak));
  setText("meal", runt(mealSize()));
  const progress = Math.min(1, status.open.plates / status.target);
  $("progress-bar").style.width = `${(progress * 100).toFixed(1)}%`;
  $("progress").setAttribute("aria-valuemax", status.target);
  $("progress").setAttribute("aria-valuenow", status.open.plates);
  setText("supply", runt(status.total_supply));
  setText("kennel", runt(status.kennel));
  setText("minted", runt(status.minted));
  setText("burned", runt(status.burned));
  setText("coin-name", COIN);
  const toSpeedrun = COIN !== SPEEDRUN;
  setText("coin-switch", toSpeedrun ? "See a full round in 5 minutes" : "Back to the main coin");
  $("coin-switch").href = toSpeedrun ? `?coin=${SPEEDRUN}` : "?";
  $("settle").hidden = !over;
  $("clock-label").textContent = over ? "Round over" : "Ends in";
  const census = status.census;
  setText("census-nc", census ? `${census.nakamoto}, in round ${census.round + 1}` : "not counted yet");
  setText("census-count", census ? census.validators : "–");
  setText("census-eff", census ? (Number(census.effective_x100) / 100).toFixed(2) : "–");
  setText("census-best", status.best_nakamoto || "–");
  setText("bounty", runt(rules.census_bounty));
  $("rollcall").disabled = status.open.nakamoto !== 0;
  $("rollcall").textContent = status.open.nakamoto !== 0 ? `Counted this round: ${status.open.nakamoto}` : "Take the roll call";
  setText("k-cut", bpsText(rules.kennel_bps));
  setText("k-spoil", bpsText(rules.spoil_bps));
  setText("k-fall", bpsText(rules.nakamoto_down_burn_bps));
  setText("k-rise", `+${bpsText(rules.nakamoto_up_bonus_bps)}`);
  tick();
}

function tick() {
  const { status } = state;
  if (!status) return;
  const left = Number(BigInt(status.round_ends_at) / 1_000_000n) - Date.now();
  const over = status.clock_round > status.open.round;
  setText("countdown", over ? "0:00" : duration(left / 1000));
}

function kindOf(v, data) {
  if (v.rank <= data.nakamoto) return "big";
  if (state.net === "testnet") {
    const hunger = state.hungers.get(v.id);
    return hunger?.can_feed ? "runt" : "other";
  }
  return v.id.endsWith(".poolv1.near") && v.share < 0.01 ? "runt" : "other";
}

function renderLitter(force = false) {
  const data = state[state.net];
  if (!data) return;
  const rows = data.list.map((v, i) => {
    const row = { ...v, rank: i + 1 };
    row.kind = kindOf(row, data);
    row.detail = `${nearText(v.stake)} NEAR, ${row.kind === "big" ? "big dog" : row.kind === "runt" ? "can be fed" : "cannot be fed"}`;
    return row;
  });
  // Rebuilding the table on every poll would flicker and could swallow a click, so only do it when something changed.
  const fed = state.me?.plate && state.me.plate.round === state.status?.open.round;
  const signature = JSON.stringify([state.net, Boolean(fed), rows.map((r) => [r.id, r.kind, String(r.stake), r.uptime, state.hungers.get(r.id)?.fed_near_this_round, state.hungers.get(r.id)?.hunger_bps, state.fees.get(r.id)])]);
  if (!force && signature === state.litterSignature) return;
  state.litterSignature = signature;
  drawLitter($("litter-chart"), rows, data.nakamoto, data.haltingShare);
  const coin = state.net === "testnet";
  document.querySelectorAll(".net-coin").forEach((cell) => (cell.hidden = !coin));
  $("litter-sub").textContent = coin
    ? "Every active validator on testnet, biggest first. Red ones hold a third of the stake together. Green ones can be fed."
    : "Every active validator on mainnet, biggest first. Read-only: RUNT runs on testnet for this hackathon.";
  const body = $("litter-table").tBodies[0];
  const ordered = [...rows].sort((a, b) => {
    const order = { runt: 0, other: 1, big: 2 };
    return order[a.kind] - order[b.kind] || (a.kind === "runt" ? Number(a.stake - b.stake) : Number(b.stake - a.stake));
  });
  const capBps = state.rules?.size_cap_bps ?? 100;
  const fedAlready = state.me?.plate && state.me.plate.round === state.status?.open.round;
  body.replaceChildren(
    ...ordered.map((row) => {
      const tr = document.createElement("tr");
      const name = make("td", "name", row.id);
      name.title = row.id;
      tr.appendChild(name);
      tr.appendChild(make("td", "num", nearText(row.stake)));
      tr.appendChild(make("td", "num", percent(row.share)));
      const hungerCell = document.createElement("td");
      if (row.kind === "big") hungerCell.appendChild(make("span", "badge big", "Big dog"));
      else if (row.kind === "other") hungerCell.appendChild(make("span", "badge other", row.share >= capBps / 10_000 ? "Too big" : "Not accepted"));
      else {
        const value = coin ? state.hungers.get(row.id).hunger_bps / 10_000 : hungerOf(row.share, capBps);
        const wrap = make("span", "hunger");
        const track = make("span", "track");
        const fill = document.createElement("i");
        fill.style.width = `${(value * 100).toFixed(0)}%`;
        track.appendChild(fill);
        wrap.append(track, make("b", "", `${(value * 100).toFixed(0)}%`));
        hungerCell.appendChild(wrap);
      }
      tr.appendChild(hungerCell);
      if (coin) {
        const hunger = state.hungers.get(row.id);
        tr.appendChild(make("td", "num", hunger ? commas(BigInt(hunger.fed_near_this_round)) : "–"));
      }
      tr.appendChild(make("td", "num", row.uptime === null ? "new" : `${(row.uptime * 100).toFixed(1)}%`));
      if (coin) {
        const fee = state.fees.get(row.id);
        tr.appendChild(make("td", "num", fee === undefined || fee === null ? "–" : `${(fee * 100).toFixed(0)}%`));
        const action = make("td");
        if (row.kind === "runt") {
          const button = make("button", "feed", fedAlready ? "Fed" : "Feed");
          button.disabled = Boolean(fedAlready);
          button.dataset.pool = row.id;
          button.addEventListener("click", () => openFeed(row));
          action.appendChild(button);
        }
        tr.appendChild(action);
      }
      return tr;
    }),
  );
}

const KINDS = {
  Meal: ["hit", "Mint", "meal share"],
  Kennel: ["hit", "Mint", "Kennel's cut"],
  Spoil: ["miss", "Burn", "food spoiled"],
  Goliath: ["miss", "Burn", "count fell"],
  Bounty: ["idle", "Paid", "roll call"],
  Unregister: ["miss", "Burn", "account left"],
};

function renderEvents(lines) {
  $("events-empty").hidden = lines.length > 0;
  $("events").tBodies[0].replaceChildren(
    ...lines.map((line) => {
      const [tone, verb, note] = KINDS[line.kind];
      const tr = document.createElement("tr");
      const what = document.createElement("td");
      what.append(make("span", `badge ${tone}`, verb), document.createTextNode(` ${note}`));
      tr.appendChild(what);
      tr.appendChild(make("td", "num", runt(line.amount)));
      const who = make("td", "name", line.account === COIN ? "Kennel" : line.account);
      who.title = line.account;
      tr.appendChild(who);
      tr.appendChild(make("td", "num", commas(line.round + 1)));
      return tr;
    }),
  );
}

function renderRounds(rows) {
  $("rounds-empty").hidden = rows.length > 0;
  $("rounds").tBodies[0].replaceChildren(
    ...rows.map(({ line, claimed }) => {
      const tr = document.createElement("tr");
      const span = line.through > line.round ? `${commas(line.round + 1)}–${commas(line.through + 1)}` : commas(line.round + 1);
      tr.appendChild(make("td", "", span));
      const result = document.createElement("td");
      result.appendChild(line.hit ? make("span", "badge hit", "Hit") : line.plates === 0 ? make("span", "badge idle", "Nobody came") : make("span", "badge miss", "Missed"));
      tr.appendChild(result);
      tr.appendChild(make("td", "num", `${line.plates} of ${line.target}`));
      tr.appendChild(make("td", "num", commas(line.feeders)));
      const pot = make("td", "num", runt(line.pot));
      pot.title = `${runt(claimed)} claimed so far`;
      tr.appendChild(pot);
      tr.appendChild(make("td", "num", runt(line.burned)));
      tr.appendChild(make("td", "num", line.nakamoto || "–"));
      return tr;
    }),
  );
}

const runts = (n) => `${n} ${n === 1 ? "runt" : "runts"}`;

function renderRules() {
  const r = state.rules;
  if (!r) return;
  const items = [
    ["Round length", humanSeconds(r.round_seconds)],
    ["Meal", `${runt(r.meal)} RUNT`],
    ["Streak bonus", `+${bpsText(r.streak_step_bps)} a round, up to ${r.streak_cap}`],
    ["Kennel's cut of a meal", bpsText(r.kennel_bps)],
    ["Kennel burned on a miss", bpsText(r.spoil_bps)],
    ["Target at launch", runts(r.target_start)],
    ["Target can't fall below", runts(r.target_floor)],
    ["Target after a hit", `+${bpsText(r.target_up_bps)}`],
    ["Target after a miss", `−${bpsText(r.target_down_bps)}`],
    ["A validator stops being a runt at", `${bpsText(r.size_cap_bps)} of all stake`],
    ["Least stake that can feed", `${commas(r.min_stake_near)} NEAR`],
    ["Counts in full, per runt per round", `${commas(r.full_bite_near)} NEAR`],
    ["Roll call bounty", `${runt(r.census_bounty)} RUNT`],
    ["Meal bonus if the count rises", `+${bpsText(r.nakamoto_up_bonus_bps)}`],
    ["Kennel burned if the count falls", bpsText(r.nakamoto_down_burn_bps)],
    ["Accepted staking pools", r.pool_suffixes.join(", ")],
  ];
  $("rules-list").replaceChildren(
    ...items.map(([label, value]) => {
      const row = document.createElement("div");
      row.append(make("dt", "", label), make("dd", "", value));
      return row;
    }),
  );
}

function renderMe() {
  const who = account();
  const connected = Boolean(who);
  $("plate-empty").hidden = connected;
  $("plate").hidden = !connected;
  $("wallet").textContent = connected ? who : "Connect wallet";
  if (!connected || !state.me) {
    $("claim").hidden = true;
    return;
  }
  const { plate, claimable, balance } = state.me;
  setText("me", who);
  const thisRound = plate && plate.round === state.status?.open.round;
  setText("my-plate", plate?.pending ? "waiting for the pool…" : thisRound ? `Fed ${plate.pool.split(".")[0]}, bite ${(Number(plate.bite) / 1e6).toFixed(2)}` : "not fed yet");
  setText("claimable", `${runt(claimable)} RUNT`);
  setText("balance", `${runt(balance)} RUNT`);
  $("claim").hidden = claimable === 0n;
}

function renderSimTable() {
  $("sim-table").tBodies[0].replaceChildren(
    ...Object.values(simData.scenarios).map((s) => {
      const tr = document.createElement("tr");
      tr.appendChild(make("td", "", s.label));
      const m = s.summary;
      tr.appendChild(make("td", "num", `${Math.round(m.hit_rate * 100)}%`));
      tr.appendChild(make("td", "num", String(Math.round(m.best_streak))));
      tr.appendChild(make("td", "num", (m.supply / 1e6).toFixed(1)));
      tr.appendChild(make("td", "num", (m.burned / 1e6).toFixed(1)));
      tr.appendChild(make("td", "num", String(Math.round(m.final_nc))));
      tr.appendChild(make("td", "num", commas(Math.round(m.moved_near))));
      return tr;
    }),
  );
}

async function runChecks() {
  const box = $("checks");
  const line = (state_, text) => {
    const li = document.createElement("li");
    li.append(make("span", state_ === true ? "ok" : state_ === false ? "no" : "wait", state_ === true ? "PASS" : state_ === false ? "FAIL" : "INFO"), document.createTextNode(text));
    return li;
  };
  try {
    const [keys, info, supply] = await Promise.all([accessKeys(NETWORK, COIN), accountInfo(NETWORK, COIN), view(NETWORK, COIN, "ft_total_supply")]);
    const status = state.status;
    const list = [
      line(keys.length === 0, keys.length === 0 ? "The coin's account has no access keys, so nobody can change its code or rules." : `The coin's account still has ${keys.length} access ${keys.length === 1 ? "key" : "keys"}, so its code and rules could still be changed.`),
      line(BigInt(supply) === BigInt(status.minted) - BigInt(status.burned), `Supply is ${runt(supply)} RUNT, which is everything minted minus everything burned.`),
      line(null, `The code hash is ${info.code_hash}. Compare it with a build of this repository.`),
    ];
    const explorer = make("li");
    const link = make("a", "", `Open ${COIN} in an explorer`);
    link.href = `${EXPLORER[NETWORK]}/address/${COIN}`;
    link.target = "_blank";
    link.rel = "noopener";
    explorer.appendChild(link);
    list.push(explorer);
    box.replaceChildren(...list);
  } catch (error) {
    box.replaceChildren(line(null, `The coin ${COIN} could not be read: ${friendly(error)}`));
  }
}

// ---- Actions -------------------------------------------------------------------------------------

const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function afterTx() {
  await wait(1500);
  await refresh();
}

function fullBite() {
  return state.rules.full_bite_near;
}

function updateBitePreview() {
  const target = state.feedTarget;
  if (!target) return;
  const extra = Number($("fd-amount").value || 0);
  const stake = target.stakedNear + (target.needsMore ? extra : 0);
  const bite = biteOf(target.hunger, target.fed, stake, fullBite());
  setText("fd-bite", bite > 0 ? bite.toFixed(2) : "–");
}

async function openFeed(row) {
  if (!account()) {
    connect();
    return;
  }
  const hunger = state.hungers.get(row.id);
  state.feedTarget = { pool: row.id, hunger: hunger.hunger_bps / 10_000, fed: Number(hunger.fed_near_this_round), stakedNear: 0, needsMore: false };
  setText("fd-title", `Feed ${row.id.split(".")[0]}`);
  setText("fd-pool", row.id);
  setText("fd-hunger", `${(state.feedTarget.hunger * 100).toFixed(0)}%`);
  setText("fd-staked", "checking…");
  setText("fd-bite", "–");
  $("fd-go").disabled = true;
  $("fd-amount-row").hidden = true;
  $("fd-note").textContent = "";
  $("feed-dialog").showModal();
  try {
    const staked = BigInt(await view(NETWORK, row.id, "get_account_staked_balance", { account_id: account() }));
    const stakedNear = Number(staked / ONE_NEAR);
    const minimum = state.rules.min_stake_near;
    state.feedTarget.stakedNear = stakedNear;
    state.feedTarget.needsMore = stakedNear < minimum;
    setText("fd-staked", `${nearText(staked, 2)} NEAR`);
    if (state.feedTarget.needsMore) {
      $("fd-amount-row").hidden = false;
      $("fd-amount").value = String(minimum - stakedNear + 0.1 > 0 ? Math.ceil((minimum - stakedNear) * 10 + 1) / 10 : 1);
      $("fd-go").textContent = "Stake, then feed";
      $("fd-note").textContent = `Feeding needs at least ${minimum} NEAR staked here. Your NEAR goes to the validator's pool, not to this coin, and takes four epochs, about 30 hours, to unstake.`;
    } else {
      $("fd-go").textContent = "Feed";
      $("fd-note").textContent = "This only reads your stake. Your NEAR doesn't move.";
    }
    $("fd-go").disabled = false;
    updateBitePreview();
  } catch (error) {
    $("fd-note").textContent = `Couldn't read this pool: ${friendly(error)}`;
  }
}

async function feedNow() {
  const target = state.feedTarget;
  const who = account();
  try {
    if (target.needsMore) {
      const amount = BigInt(Math.round(Number($("fd-amount").value) * 1000)) * 10n ** 21n;
      toast("Confirm the stake in your wallet…", 60000);
      await send(target.pool, [call("deposit_and_stake", {}, 150, amount)]);
    }
    const registered = await view(NETWORK, COIN, "storage_balance_of", { account_id: who });
    const actions = [];
    if (!registered) {
      const bounds = await view(NETWORK, COIN, "storage_balance_bounds");
      actions.push(call("storage_deposit", { registration_only: true }, 30, BigInt(bounds.min)));
    }
    actions.push(call("feed", { pool: target.pool }, 60));
    toast("Confirm the feed in your wallet…", 60000);
    await send(COIN, actions);
    for (let i = 0; i < 12; i++) {
      await wait(1200);
      const plate = await view(NETWORK, COIN, "plate_of", { account_id: who });
      if (plate && !plate.pending && plate.round === state.status.open.round) {
        toast(`Fed. Your bite is ${(Number(plate.bite) / 1e6).toFixed(2)}.`, 7000);
        await afterTx();
        return;
      }
      if (!plate) break;
    }
    toast("The pool didn't confirm enough stake. Nothing was recorded.", 7000);
    await afterTx();
  } catch (error) {
    toast(friendly(error), 8000);
  }
}

async function claimNow() {
  try {
    toast("Confirm in your wallet…", 60000);
    await send(COIN, [call("claim", {}, 60)]);
    toast("Claimed. Your meal has been minted.", 7000);
    await afterTx();
  } catch (error) {
    toast(friendly(error), 8000);
  }
}

async function settleNow() {
  if (!account()) {
    connect();
    return;
  }
  try {
    toast("Confirm in your wallet…", 60000);
    await send(COIN, [call("settle", {}, 120)]);
    toast("Round closed.", 5000);
    await afterTx();
  } catch (error) {
    toast(friendly(error), 8000);
  }
}

async function rollCallNow() {
  if (!account()) {
    connect();
    return;
  }
  try {
    await loadValidators();
    const list = state.testnet.list.map((v) => v.id);
    toast(`Counting ${list.length} validators. Confirm in your wallet…`, 60000);
    await send(COIN, [call("roll_call", { validators: list }, 40 + Math.ceil(list.length * 1.3))]);
    toast("Roll call taken. The contract checked it against the protocol.", 7000);
    await afterTx();
  } catch (error) {
    toast(friendly(error), 8000);
  }
}

// ---- Start ----------------------------------------------------------------------------------------

let refreshing = false;
let refreshAgain = false;
async function refresh() {
  if (refreshing) {
    refreshAgain = true;
    return;
  }
  refreshing = true;
  try {
    if (!state.testnet) await loadValidators();
    await loadCoin();
    await Promise.all([loadLogs(), loadMe()]);
    renderRound();
    renderLitter();
    renderMe();
    $("settle").disabled = false;
  } catch (error) {
    if (/does not exist|unknown|CodeDoesNotExist/i.test(String(error))) setText("round-label", "Coin not found");
    console.warn(error);
  } finally {
    refreshing = false;
    if (refreshAgain) {
      refreshAgain = false;
      refresh();
    }
  }
}

function setNet(net) {
  state.net = net;
  $("net-testnet").classList.toggle("on", net === "testnet");
  $("net-mainnet").classList.toggle("on", net === "mainnet");
  renderLitter();
}

$("wallet").addEventListener("click", () => (account() ? disconnect() : connect()));
$("plate-connect").addEventListener("click", () => connect());
$("claim").addEventListener("click", claimNow);
$("settle").addEventListener("click", settleNow);
$("rollcall").addEventListener("click", rollCallNow);
$("net-testnet").addEventListener("click", () => setNet("testnet"));
$("net-mainnet").addEventListener("click", () => setNet("mainnet"));
$("fd-amount").addEventListener("input", updateBitePreview);
$("feed-form").addEventListener("submit", (event) => {
  if (event.submitter?.id === "fd-go") feedNow();
});

// The wallet reports "no account" as soon as this runs, before any chain data has arrived.
let started = false;
onAccount(() => {
  renderMe();
  if (started) refresh();
});

let resizeTimer;
new ResizeObserver(() => {
  clearTimeout(resizeTimer);
  resizeTimer = setTimeout(() => {
    renderLitter(true);
    drawSupply($("supply-chart"), simData);
  }, 120);
}).observe(document.body);

async function start() {
  renderSimTable();
  drawSupply($("supply-chart"), simData);
  await loadValidators().catch((error) => console.warn(error));
  await refresh();
  started = true;
  restore();
  renderRules();
  await loadFees();
  renderLitter();
  runChecks();
  setInterval(refresh, 10000);
  setInterval(() => loadValidators().then(renderLitter).catch(() => {}), 120000);
  setInterval(tick, 1000);
}

start();
