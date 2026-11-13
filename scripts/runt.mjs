// Everyday actions from the command line.
//   node scripts/runt.mjs status [--coin <id>]                       no account needed
//   node scripts/runt.mjs litter [--coin <id>]                       no account needed
//   node scripts/runt.mjs stake <pool> <near> --as <account>
//   node scripts/runt.mjs feed <pool> --as <account> [--coin <id>]
//   node scripts/runt.mjs claim --as <account> [--coin <id>]
//   node scripts/runt.mjs settle --as <account> [--coin <id>]
//   node scripts/runt.mjs rollcall --as <account> [--coin <id>]
import { actions } from "near-api-js";
import { DEFAULT_COIN, ONE_NEAR, TGAS, failureOf, near, parseArgs, runt, signer, txLink, validators, view } from "./lib.mjs";

const { positional, flags } = parseArgs();
const [command, ...rest] = positional;
const coin = flags.coin ?? DEFAULT_COIN;

async function send(receiverId, list, label) {
  const account = signer(flags.as);
  const result = await account.signAndSendTransaction({ receiverId, actions: list, throwOnFailure: false });
  const failed = failureOf(result);
  const hash = result.transaction.hash;
  if (failed) {
    console.error(`${label} FAILED: ${failed}\n  ${txLink(hash)}`);
    process.exit(1);
  }
  console.log(`${label}\n  ${txLink(hash)}`);
  return result;
}

async function status() {
  const [s, rules] = await Promise.all([view(coin, "status"), view(coin, "rules")]);
  const left = Math.max(0, Number(BigInt(s.round_ends_at) / 1_000_000n) - Date.now());
  const over = s.clock_round > s.open.round;
  console.log(`Coin            ${coin}`);
  console.log(`Round           ${s.open.round + 1}${over ? " (time is up, waiting to be closed)" : `, ends in ${Math.round(left / 1000)} s`}`);
  console.log(`Runts fed       ${s.open.plates} of ${s.target} needed, by ${s.open.feeders} feeders`);
  console.log(`Streak          ${s.streak}, Best in Show ${s.best_streak}`);
  console.log(`Supply          ${runt(s.total_supply)} RUNT (minted ${runt(s.minted)}, burned ${runt(s.burned)})`);
  console.log(`Kennel          ${runt(s.kennel)} RUNT`);
  console.log(`Nakamoto        last ${s.last_nakamoto || "none"}, best ${s.best_nakamoto || "none"}${s.census ? `, counted in round ${s.census.round + 1} by ${s.census.taken_by}` : ""}`);
  console.log(`Round length    ${rules.round_seconds} s`);
}

async function litter() {
  const { list, nakamoto } = await validators();
  const pools = list.map((v) => v.id);
  const hungers = await view(coin, "hungers", { pools });
  console.log(`Nakamoto number on this network: ${nakamoto}\n`);
  console.log("Validator".padEnd(36), "Stake".padStart(14), "Share".padStart(8), "Hunger".padStart(8), "  Can feed");
  list.forEach((v, i) => {
    const h = hungers[i];
    console.log(v.id.padEnd(36), near(v.stake, 0).padStart(14), `${(h.share_ppm / 10_000).toFixed(2)}%`.padStart(8), `${(h.hunger_bps / 100).toFixed(0)}%`.padStart(8), " ", h.can_feed ? "yes" : `no: ${h.why_not}`);
  });
}

async function stake(pool, amount) {
  const yocto = (BigInt(Math.round(Number(amount) * 1000)) * ONE_NEAR) / 1000n;
  await send(pool, [actions.functionCall("deposit_and_stake", {}, 150n * TGAS, yocto)], `Staked ${amount} NEAR with ${pool}`);
  const staked = await view(pool, "get_account_staked_balance", { account_id: flags.as });
  console.log(`  Your stake there is now ${near(staked)} NEAR`);
}

async function feed(pool) {
  const registered = await view(coin, "storage_balance_of", { account_id: flags.as });
  const list = [];
  if (!registered) {
    const bounds = await view(coin, "storage_balance_bounds");
    list.push(actions.functionCall("storage_deposit", { registration_only: true }, 30n * TGAS, BigInt(bounds.min)));
  }
  list.push(actions.functionCall("feed", { pool }, 60n * TGAS, 0n));
  await send(coin, list, `Fed ${pool}`);
  const plate = await view(coin, "plate_of", { account_id: flags.as });
  console.log(plate && !plate.pending ? `  Bite ${(Number(plate.bite) / 1e6).toFixed(2)} in round ${plate.round + 1}` : "  The pool did not confirm enough stake, so nothing was recorded.");
}

async function claim() {
  const before = await view(coin, "ft_balance_of", { account_id: flags.as });
  await send(coin, [actions.functionCall("claim", {}, 60n * TGAS, 0n)], "Claimed");
  const after = await view(coin, "ft_balance_of", { account_id: flags.as });
  console.log(`  Received ${runt(BigInt(after) - BigInt(before))} RUNT. Balance ${runt(after)} RUNT`);
}

async function settle() {
  await send(coin, [actions.functionCall("settle", {}, 120n * TGAS, 0n)], "Settled");
  await status();
}

async function rollcall() {
  const { list, nakamoto } = await validators();
  const ids = list.map((v) => v.id);
  const gas = BigInt(40 + Math.ceil(ids.length * 1.3));
  await send(coin, [actions.functionCall("roll_call", { validators: ids }, gas * TGAS, 0n)], `Roll call of ${ids.length} validators, ${gas} TGas`);
  const s = await view(coin, "status");
  console.log(`  Contract says ${s.census.nakamoto}. Counted from RPC: ${nakamoto}. Effective validators ${(Number(s.census.effective_x100) / 100).toFixed(2)}.`);
}

const commands = { status, litter, stake, feed, claim, settle, rollcall };
if (!commands[command]) {
  console.error("usage: node scripts/runt.mjs status|litter|stake|feed|claim|settle|rollcall ...");
  process.exit(1);
}
if (["stake", "feed", "claim", "settle", "rollcall"].includes(command) && !flags.as) {
  console.error("This command signs a transaction, so it needs --as <account>.");
  process.exit(1);
}
await commands[command](...rest);
