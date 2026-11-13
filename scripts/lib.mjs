// Shared helpers. Reading needs no key and no account. Signing reads a key from the NEAR CLI's
// legacy keychain (~/.near-credentials), or RUNT_PRIVATE_KEY, and never prints it.
import { existsSync, readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { Account, JsonRpcProvider } from "near-api-js";

export const NETWORK = process.env.NEAR_NETWORK ?? "testnet";
export const DEFAULT_COIN = process.env.RUNT_COIN ?? "coin.runt-hq.testnet";
export const EXPLORER = NETWORK === "mainnet" ? "https://nearblocks.io" : "https://testnet.nearblocks.io";
export const TGAS = 1_000_000_000_000n;
export const ONE_NEAR = 10n ** 24n;
export const ONE_RUNT = 10n ** 18n;

const ENDPOINTS = {
  testnet: ["https://near-testnet.drpc.org", "https://test.rpc.fastnear.com", "https://rpc.testnet.fastnear.com"],
  mainnet: ["https://rpc.mainnet.fastnear.com", "https://near.drpc.org", "https://free.rpc.fastnear.com"],
};
const urls = (network) => (process.env.NEAR_RPC ? [process.env.NEAR_RPC] : ENDPOINTS[network]);
// These are real answers from the chain. Any other error is an endpoint's problem, so try the next one.
const ANSWERS = new Set(["UNKNOWN_ACCOUNT", "UNKNOWN_ACCESS_KEY", "NO_CONTRACT_CODE", "CONTRACT_EXECUTION_ERROR"]);

export function parseArgs() {
  const positional = [];
  const flags = {};
  const raw = process.argv.slice(2);
  for (let i = 0; i < raw.length; i++) {
    if (raw[i].startsWith("--")) {
      const next = raw[i + 1];
      if (next === undefined || next.startsWith("--")) flags[raw[i].slice(2)] = true;
      else flags[raw[i].slice(2)] = raw[++i];
    } else positional.push(raw[i]);
  }
  return { positional, flags };
}

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

export async function rpc(method, params, network = NETWORK) {
  let lastError;
  for (const [attempt, url] of [...urls(network), urls(network)[0]].entries()) {
    if (attempt) await sleep(500 * attempt);
    try {
      const response = await fetch(url, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ jsonrpc: "2.0", id: "runt", method, params }),
      });
      const body = await response.json();
      if (body.result?.error) throw Object.assign(new Error(body.result.error), { final: true });
      if (body.error) {
        const text = body.error.data ?? body.error.cause?.name ?? body.error.message ?? JSON.stringify(body.error);
        throw Object.assign(new Error(String(text)), { final: ANSWERS.has(body.error.cause?.name) });
      }
      return body.result;
    } catch (error) {
      if (error.final) throw error;
      lastError = error;
    }
  }
  throw lastError;
}

export async function view(contract, method, args = {}, network = NETWORK) {
  const result = await rpc("query", {
    request_type: "call_function",
    finality: "optimistic",
    account_id: contract,
    method_name: method,
    args_base64: Buffer.from(JSON.stringify(args)).toString("base64"),
  }, network);
  return JSON.parse(Buffer.from(result.result).toString());
}

export const accountInfo = (id, network = NETWORK) => rpc("query", { request_type: "view_account", finality: "final", account_id: id }, network);

export const accessKeys = async (id, network = NETWORK) =>
  (await rpc("query", { request_type: "view_access_key_list", finality: "final", account_id: id }, network)).keys;

/** Active validators, biggest first, with ties in account order: the order `roll_call` requires. */
export async function validators(network = NETWORK) {
  const result = await rpc("validators", [null], network);
  const list = result.current_validators.map((v) => ({ id: v.account_id, stake: BigInt(v.stake) }));
  list.sort((a, b) => (b.stake > a.stake ? 1 : b.stake < a.stake ? -1 : a.id < b.id ? -1 : 1));
  const total = list.reduce((sum, v) => sum + v.stake, 0n);
  let running = 0n;
  let nakamoto = 0;
  for (const [i, v] of list.entries()) {
    running += v.stake;
    if (running * 3n > total) {
      nakamoto = i + 1;
      break;
    }
  }
  return { list, total, nakamoto };
}

export function secretKeyFor(accountId) {
  if (process.env.RUNT_PRIVATE_KEY) return process.env.RUNT_PRIVATE_KEY;
  const file = join(homedir(), ".near-credentials", NETWORK, `${accountId}.json`);
  if (!existsSync(file)) throw new Error(`No key for ${accountId}. Create the account with the NEAR CLI (save-to-legacy-keychain), or set RUNT_PRIVATE_KEY.`);
  return JSON.parse(readFileSync(file, "utf8")).private_key;
}

// Transactions go through FastNEAR. Other endpoints can hang while waiting on cross-contract calls.
const SEND = { testnet: "https://test.rpc.fastnear.com", mainnet: "https://rpc.mainnet.fastnear.com" };
export const providerFor = () => new JsonRpcProvider({ url: process.env.NEAR_RPC ?? SEND[NETWORK] });

export const signer = (accountId) => new Account(accountId, providerFor(), secretKeyFor(accountId));

export const near = (yocto, digits = 2) => (Number(BigInt(yocto) / 10n ** 20n) / 10_000).toLocaleString("en-US", { maximumFractionDigits: digits });

export function runt(units) {
  const value = BigInt(units);
  const whole = value / ONE_RUNT;
  return whole >= 1000n || value === 0n ? whole.toLocaleString("en-US") : (Number(value / 10n ** 14n) / 10_000).toLocaleString("en-US", { maximumFractionDigits: 2 });
}

export const txLink = (hash) => `${EXPLORER}/txns/${hash}`;

/** The first contract panic in a finished transaction, or null if it succeeded. */
export function failureOf(result) {
  for (const outcome of [result.transaction_outcome, ...(result.receipts_outcome ?? [])]) {
    const failure = outcome?.outcome?.status?.Failure;
    if (failure) return JSON.stringify(failure);
  }
  return null;
}
