// Launches a RUNT coin: creates the account, deploys the code, sets the rules once, removes the key.
//   node scripts/launch.mjs --parent <account> --name <label> [--preset main|speedrun|mainnet] [--baseline <n>] [--room-kb <n>] [--keep-key]
import { readFileSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { KeyPair, actions } from "near-api-js";
import { EXPLORER, NETWORK, ONE_NEAR, ONE_RUNT, TGAS, accessKeys, failureOf, parseArgs, signer } from "./lib.mjs";

const TESTNET_POOLS = ["pool.f863973.m0"];

// The values picked from the simulation. `python sim/runt_sim.py picks` shows why.
const launch = {
  round_seconds: 86_400,
  meal: String(1_000_000n * ONE_RUNT),
  streak_step_bps: 200,
  streak_cap: 30,
  kennel_bps: 2_000,
  spoil_bps: 2_500,
  target_start: 5,
  target_floor: 3,
  target_up_bps: 1_000,
  target_down_bps: 3_000,
  size_cap_bps: 100,
  min_stake_near: 10,
  full_bite_near: 1_000,
  census_bounty: String(5_000n * ONE_RUNT),
  nakamoto_up_bonus_bps: 20_000,
  nakamoto_down_burn_bps: 5_000,
  baseline_nakamoto: 0,
  // poolv1.near has no access keys and all its pools run one code hash. pool.near still has keys
  // and its pools vary, so its name alone can't prove the pool is honest.
  pool_suffixes: ["poolv1.near"],
};

const PRESETS = {
  mainnet: launch,
  // Testnet has nine feedable pools and only faucet money, so the stake floor is 1 NEAR and the
  // target starts small. Everything else matches the mainnet rules.
  main: { ...launch, min_stake_near: 1, target_start: 2, target_floor: 1, pool_suffixes: TESTNET_POOLS },
  // The same mechanics in 5-minute rounds, so one person can see a whole cycle.
  speedrun: { ...launch, round_seconds: 300, target_start: 2, target_floor: 1, min_stake_near: 1, pool_suffixes: TESTNET_POOLS },
};

const { flags } = parseArgs();
if (!flags.parent || !flags.name) {
  console.error("usage: node scripts/launch.mjs --parent <account> --name <label> [--preset main|speedrun|mainnet] [--baseline <n>] [--room-kb <n>] [--keep-key]");
  process.exit(1);
}

const rules = { ...PRESETS[flags.preset ?? "main"] };
if (flags.baseline !== undefined) rules.baseline_nakamoto = Number(flags.baseline);
if (flags["round-seconds"] !== undefined) rules.round_seconds = Number(flags["round-seconds"]);
if (flags["target-start"] !== undefined) rules.target_start = Number(flags["target-start"]);
const wasm = readFileSync(flags.wasm ?? join(import.meta.dirname, "..", "contract", "res", "runt.wasm"));
const id = `${flags.name}.${flags.parent}`;
// Code, plus room for the rounds table and the log. A hit round adds about 230 bytes. A mainnet
// launch sets aside 5 NEAR, about six years of daily rounds. Anyone can top it up later.
const extra = flags.preset === "mainnet" ? 5n * ONE_NEAR : BigInt(flags["room-kb"] ?? 60) * 1000n * 10n ** 19n;
const reserve = BigInt(wasm.length) * 10n ** 19n + extra;

const parent = signer(flags.parent);
const keyPair = KeyPair.fromRandom("ed25519");
const publicKey = keyPair.getPublicKey();

console.log(`Creating ${id}: ${wasm.length} bytes of code, ${Number(reserve * 1000n / ONE_NEAR) / 1000} NEAR for storage.`);
const result = await parent.signAndSendTransaction({
  receiverId: id,
  actions: [
    actions.createAccount(),
    actions.transfer(reserve),
    actions.addFullAccessKey(publicKey),
    actions.deployContract(wasm),
    actions.functionCall("new", { rules }, 30n * TGAS, 0n),
  ],
  throwOnFailure: false,
});
const failed = failureOf(result);
if (failed) {
  console.error(`Launch failed: ${failed}`);
  process.exit(1);
}
console.log(`Launched: ${EXPLORER}/address/${id}\n  tx ${result.transaction.hash}`);

if (flags["keep-key"]) {
  // Only for throwaway tests, so the account can be deleted and its NEAR recovered.
  const file = join(homedir(), ".near-credentials", NETWORK, `${id}.json`);
  writeFileSync(file, JSON.stringify({ account_id: id, public_key: publicKey.toString(), private_key: keyPair.toString() }), { mode: 0o600 });
  console.log("Key kept. This account is NOT locked.");
  process.exit(0);
}

process.env.RUNT_PRIVATE_KEY = keyPair.toString();
const self = signer(id);
delete process.env.RUNT_PRIVATE_KEY;
const removed = await self.signAndSendTransaction({ receiverId: id, actions: [actions.deleteKey(publicKey)] });
let left = await accessKeys(id);
for (let i = 0; i < 12 && left.length !== 0; i++) {
  await new Promise((resolve) => setTimeout(resolve, 1000));
  left = await accessKeys(id);
}
if (left.length !== 0) {
  console.error(`Lock failed: ${left.length} key(s) remain on ${id}.`);
  process.exit(1);
}
console.log(`Locked: ${id} has no access keys, so nobody can change its code or rules.\n  tx ${removed.transaction.hash}`);
