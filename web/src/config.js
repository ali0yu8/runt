const params = new URLSearchParams(location.search);

/** The coin this page shows. `?coin=` opens any other RUNT coin on testnet. */
export const COIN = params.get("coin") || "coin.runt-hq.testnet";
export const NETWORK = "testnet";

export const RPC = {
  testnet: ["https://near-testnet.drpc.org", "https://test.rpc.fastnear.com", "https://rpc.testnet.fastnear.com"],
  mainnet: ["https://rpc.mainnet.fastnear.com", "https://near.drpc.org", "https://free.rpc.fastnear.com"],
};

export const EXPLORER = { testnet: "https://testnet.nearblocks.io", mainnet: "https://nearblocks.io" };

export const TGAS = 1_000_000_000_000n;
export const ONE_NEAR = 10n ** 24n;
export const ONE_RUNT = 10n ** 18n;
