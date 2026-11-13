// Checks a RUNT coin against the chain. It needs no wallet, no account and no key.
//   node scripts/verify.mjs [coin]
import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { DEFAULT_COIN, NETWORK, accessKeys, accountInfo, parseArgs, runt, rpc, validators, view } from "./lib.mjs";

const { positional } = parseArgs();
const coin = positional[0] ?? DEFAULT_COIN;
let failed = 0;

function report(ok, text) {
  console.log(`${ok ? "PASS" : "FAIL"}  ${text}`);
  if (!ok) failed++;
}
const info = (text) => console.log(`INFO  ${text}`);

function base58(bytes) {
  const alphabet = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
  let value = BigInt(`0x${Buffer.from(bytes).toString("hex")}`);
  let text = "";
  while (value > 0n) {
    text = alphabet[Number(value % 58n)] + text;
    value /= 58n;
  }
  return text;
}

async function refused(method, args, expected) {
  try {
    await view(coin, method, args);
    return false;
  } catch (error) {
    return String(error.message ?? error).includes(expected);
  }
}

// ---- NEP-141 ----------------------------------------------------------------------------------
const metadata = await view(coin, "ft_metadata");
report(metadata.spec === "ft-1.0.0" && metadata.symbol === "RUNT" && metadata.decimals === 18, `NEP-141: ${metadata.name} (${metadata.symbol}), ${metadata.decimals} decimals, spec ${metadata.spec}`);
const bounds = await view(coin, "storage_balance_bounds");
report(BigInt(bounds.min) > 0n, "NEP-145: accounts pay for their own storage");

// ---- No owner ----------------------------------------------------------------------------------
const keys = await accessKeys(coin);
report(keys.length === 0, keys.length === 0 ? "No owner: the account has no access keys, so its code and rules can't be changed" : `The account still has ${keys.length} access ${keys.length === 1 ? "key" : "keys"}, so it is not locked`);
const account = await accountInfo(coin);
const wasmFile = join(import.meta.dirname, "..", "contract", "res", "runt.wasm");
if (existsSync(wasmFile)) {
  const local = base58(createHash("sha256").update(readFileSync(wasmFile)).digest());
  report(local === account.code_hash, `The live code is this repository's build: contract/res/runt.wasm hashes to ${local}`);
} else {
  info(`Code hash ${account.code_hash}`);
}

// ---- Parameters ---------------------------------------------------------------------------------
const rules = await view(coin, "rules");
report(Number(rules.round_seconds) > 0 && BigInt(rules.meal) > 0n, `Rules readable by anyone: ${rules.round_seconds} s rounds, meal ${runt(rules.meal)} RUNT, Kennel ${rules.kennel_bps / 100}%, spoil ${rules.spoil_bps / 100}%, target ${rules.target_start} runts`);

// ---- Books -------------------------------------------------------------------------------------
const status = await view(coin, "status");
report(BigInt(status.total_supply) === BigInt(status.minted) - BigInt(status.burned), `Books balance: supply ${runt(status.total_supply)} = minted ${runt(status.minted)} - burned ${runt(status.burned)}`);
const kennelBalance = await view(coin, "ft_balance_of", { account_id: coin });
report(BigInt(kennelBalance) === BigInt(status.kennel), `The Kennel is a real token balance: ${runt(kennelBalance)} RUNT`);

// ---- The roll call -----------------------------------------------------------------------------
const set = await validators();
const ids = set.list.map((v) => v.id);
const roll = await view(coin, "check_roll_call", { validators: ids });
report(roll.nakamoto === set.nakamoto && roll.validators === ids.length, `Roll call: the contract counts ${roll.validators} validators and gets ${roll.nakamoto}, matching an independent count from RPC`);
report(await refused("check_roll_call", { validators: ids.slice(0, -1) }, "RUNT[ROLL]"), "Roll call: leaving out one validator is refused");
report(await refused("check_roll_call", { validators: [ids[1], ids[0], ...ids.slice(2)] }, "RUNT[ROLL]"), "Roll call: the wrong order is refused");
report(await refused("check_roll_call", { validators: [ids[0], ids[0], ...ids.slice(1)] }, "RUNT[ROLL]"), "Roll call: listing a validator twice is refused");
report(await refused("check_roll_call", { validators: [...ids, "not-a-validator.testnet"] }, "RUNT[ROLL]"), "Roll call: a stranger in the list is refused");

// ---- Whose word the coin takes -------------------------------------------------------------------
for (const suffix of rules.pool_suffixes) {
  const factoryKeys = await accessKeys(suffix);
  const pools = ids.filter((id) => id.endsWith(`.${suffix}`));
  const hashes = new Set();
  for (const id of pools) {
    hashes.add((await accountInfo(id)).code_hash);
    await new Promise((resolve) => setTimeout(resolve, 150));
  }
  report(factoryKeys.length === 0 && hashes.size === 1, `Pools are trustworthy by name: factory ${suffix} has ${factoryKeys.length} access keys, and its ${pools.length} active pools run ${hashes.size} version of the code`);
}

// ---- Who can be fed ---------------------------------------------------------------------------
const hungers = await view(coin, "hungers", { pools: ids });
const biggestPool = set.list.findIndex((v) => rules.pool_suffixes.some((s) => v.id.endsWith(`.${s}`)));
const biggest = hungers[biggestPool];
report(!biggest.can_feed && biggest.hunger_bps === 0, `Big dogs can't be fed: ${ids[biggestPool]} holds ${(biggest.share_ppm / 10_000).toFixed(2)}% and has no hunger`);
const runts = hungers.map((h, i) => ({ ...h, id: ids[i] })).filter((h) => h.can_feed);
report(runts.length > 0, `${runts.length} validators can be fed. Hungriest: ${[...runts].sort((a, b) => b.hunger_bps - a.hunger_bps)[0].id}`);
const ordered = runts.every((h, i) => i === 0 || h.share_ppm >= runts[i - 1].share_ppm ? h.hunger_bps <= runts[i - 1]?.hunger_bps || i === 0 : true);
report(ordered, "Smaller validators are hungrier");

const status2 = await rpc("status", []);
info(`Checked on ${NETWORK} at block ${status2.sync_info.latest_block_height}`);
console.log(failed === 0 ? "\nEverything matches." : `\n${failed} check(s) failed.`);
process.exit(failed === 0 ? 0 : 1);
