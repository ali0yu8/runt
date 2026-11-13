import { NearConnector } from "@hot-labs/near-connect";
import { NETWORK, TGAS } from "./config.js";

const connector = new NearConnector({ network: NETWORK });
const listeners = new Set();
let current = null;

function publish(accountId) {
  current = accountId;
  for (const listener of listeners) listener(accountId);
}

connector.on("wallet:signIn", (event) => publish(event.accounts?.[0]?.accountId ?? null));
connector.on("wallet:signOut", () => publish(null));

export function onAccount(listener) {
  listeners.add(listener);
  listener(current);
}

export async function restore() {
  try {
    const wallet = await connector.wallet();
    const accounts = await wallet.getAccounts();
    if (accounts.length) publish(accounts[0].accountId);
  } catch {
    // Nobody has connected yet.
  }
}

export const connect = () => connector.connect();
export const disconnect = () => connector.disconnect();
export const account = () => current;

export const call = (method, args, tgas, deposit = 0n) => ({
  type: "FunctionCall",
  params: { methodName: method, args, gas: (BigInt(tgas) * TGAS).toString(), deposit: deposit.toString() },
});

export async function send(receiverId, actions) {
  const wallet = await connector.wallet();
  return wallet.signAndSendTransaction({ receiverId, actions });
}
