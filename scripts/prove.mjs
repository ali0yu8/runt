// Tries the things a live coin must refuse, using real transactions, and prints each result.
// Unit tests can't cover these, because the guards live in the compiled contract's entry points.
//   node scripts/prove.mjs --as <account> [--coin <id>]
import { actions } from "near-api-js";
import { DEFAULT_COIN, ONE_NEAR, TGAS, failureOf, parseArgs, signer, txLink, validators, view } from "./lib.mjs";

const { flags } = parseArgs();
if (!flags.as) {
  console.error("usage: node scripts/prove.mjs --as <account> [--coin <id>]");
  process.exit(1);
}
const coin = flags.coin ?? DEFAULT_COIN;
const account = signer(flags.as);
let wrong = 0;

async function mustFail(label, method, args, expected, gas = 60n, deposit = 0n) {
  const result = await account.signAndSendTransaction({
    receiverId: coin,
    actions: [actions.functionCall(method, args, gas * TGAS, deposit)],
    throwOnFailure: false,
  });
  const failure = failureOf(result) ?? "";
  const refused = failure.includes(expected);
  if (!refused) wrong++;
  console.log(`${refused ? "REFUSED" : "NOT REFUSED"}  ${label}\n         ${txLink(result.transaction.hash)}\n         ${failure ? expected : "the call succeeded"}`);
  await new Promise((resolve) => setTimeout(resolve, 1200));
}

const set = await validators();
const ids = set.list.map((v) => v.id);
const rules = await view(coin, "rules");
const bigPool = ids.find((id) => rules.pool_suffixes.some((s) => id.endsWith(`.${s}`)));
const notPool = ids.find((id) => !rules.pool_suffixes.some((s) => id.endsWith(`.${s}`)));
const sleeping = `not-active-${Date.now()}.${rules.pool_suffixes[0]}`;

if (!(await view(coin, "storage_balance_of", { account_id: flags.as }))) {
  const bounds = await view(coin, "storage_balance_bounds");
  await account.signAndSendTransaction({ receiverId: coin, actions: [actions.functionCall("storage_deposit", { registration_only: true }, 30n * TGAS, BigInt(bounds.min))] });
}

await mustFail("Setting the rules a second time", "new", { rules }, "already been initialized");
await mustFail("A stranger faking the staking pool's answer", "on_feed", { who: flags.as, pool: bigPool, round: 0, hunger: 10000 }, "is private");
await mustFail(`Feeding a big dog (${bigPool})`, "feed", { pool: bigPool }, "RUNT[BIGDOG]");
await mustFail(`Feeding an account that isn't a factory pool (${notPool})`, "feed", { pool: notPool }, "RUNT[POOL]");
await mustFail("Feeding a pool outside the active set", "feed", { pool: sleeping }, "RUNT[ASLEEP]");
await mustFail("A roll call that leaves out one validator", "roll_call", { validators: ids.slice(0, -1) }, "RUNT[ROLL]", 100n);
await mustFail("A roll call in the wrong order", "roll_call", { validators: [ids[1], ids[0], ...ids.slice(2)] }, "RUNT[ROLL]", 100n);
await mustFail("A roll call listing a validator twice", "roll_call", { validators: [ids[0], ids[0], ...ids.slice(1)] }, "RUNT[ROLL]", 100n);
await mustFail("A transfer without 1 yoctoNEAR", "ft_transfer", { receiver_id: coin, amount: "1" }, "exactly 1 yoctoNEAR");

console.log(wrong === 0 ? "\nEverything was refused." : `\n${wrong} call(s) were not refused.`);
process.exit(wrong === 0 ? 0 : 1);
void ONE_NEAR;
