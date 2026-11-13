// Everything the page knows comes from these reads. There is no server behind it.
import { RPC } from "./config.js";

export async function rpc(network, method, params) {
  let lastError;
  // Public RPCs rate-limit bursts, so a busy endpoint gets a pause and then the next one is tried.
  for (const [attempt, url] of [...RPC[network], RPC[network][0]].entries()) {
    if (attempt) await new Promise((resolve) => setTimeout(resolve, 400 * attempt));
    try {
      const response = await fetch(url, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ jsonrpc: "2.0", id: "runt", method, params }),
      });
      const body = await response.json();
      if (body.result?.error) throw new Answer(body.result.error);
      if (body.error) {
        const text = body.error.data || body.error.cause?.name || body.error.message || JSON.stringify(body.error);
        if (ANSWERS.has(body.error.cause?.name)) throw new Answer(String(text));
        throw new Error(String(text));
      }
      return body.result;
    } catch (error) {
      if (error instanceof Answer) throw error;
      lastError = error;
    }
  }
  throw lastError;
}

/** The chain answered, and the answer is an error. Asking another endpoint won't change it. */
class Answer extends Error {}
const ANSWERS = new Set(["UNKNOWN_ACCOUNT", "UNKNOWN_ACCESS_KEY", "NO_CONTRACT_CODE", "CONTRACT_EXECUTION_ERROR"]);

export async function view(network, contract, method, args = {}) {
  const result = await rpc(network, "query", {
    request_type: "call_function",
    finality: "optimistic",
    account_id: contract,
    method_name: method,
    args_base64: btoa(JSON.stringify(args)),
  });
  return JSON.parse(new TextDecoder().decode(new Uint8Array(result.result)));
}

export const accountInfo = (network, id) => rpc(network, "query", { request_type: "view_account", finality: "final", account_id: id });

export const accessKeys = async (network, id) =>
  (await rpc(network, "query", { request_type: "view_access_key_list", finality: "final", account_id: id })).keys;

function uptime(v) {
  const pairs = [
    [v.num_produced_blocks, v.num_expected_blocks],
    [v.num_produced_chunks, v.num_expected_chunks],
    [v.num_produced_endorsements, v.num_expected_endorsements],
  ].filter(([, expected]) => expected > 0);
  if (!pairs.length) return null;
  return Math.min(...pairs.map(([done, expected]) => done / expected));
}

/** The active validator set, biggest first, in the order `roll_call` requires. */
export async function validators(network) {
  const result = await rpc(network, "validators", [null]);
  return shape(result.current_validators.map((v) => ({ id: v.account_id, stake: BigInt(v.stake), uptime: uptime(v) })));
}

/** The same figures from the saved snapshot, for when no public RPC will answer. */
export const fromSnapshot = (rows) => shape(rows.map(([id, near]) => ({ id, stake: BigInt(Math.round(near * 100)) * 10n ** 22n, uptime: null })));

function shape(list) {
  list.sort((a, b) => (b.stake > a.stake ? 1 : b.stake < a.stake ? -1 : a.id < b.id ? -1 : 1));
  const total = list.reduce((sum, v) => sum + v.stake, 0n);
  let running = 0n;
  let nakamoto = 0;
  let halting = 0n;
  list.forEach((v, i) => {
    running += v.stake;
    v.share = Number((v.stake * 1_000_000n) / total) / 1_000_000;
    if (!nakamoto && running * 3n > total) {
      nakamoto = i + 1;
      halting = running;
    }
  });
  // To make the number bigger, the top group must fall to a third or less.
  const toMove = halting - total / 3n;
  return { list, total, nakamoto, haltingShare: Number((halting * 1_000_000n) / total) / 1_000_000, toMove };
}
