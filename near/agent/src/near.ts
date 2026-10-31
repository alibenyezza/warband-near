// Shared setup: the agent account signs with a function-call key only (AGENT_PRIVATE_KEY in .env, never committed).
import { Account, JsonRpcProvider, type KeyPairString } from "near-api-js";

export const RPC = process.env.RPC ?? "https://test.rpc.fastnear.com";
export const WARBAND = process.env.WARBAND ?? "warband-band.testnet";
export const DAO = process.env.DAO ?? "dao.warband-hq.testnet";
export const AGENT = process.env.AGENT_ACCOUNT_ID ?? "keeper.warband-hq.testnet";

export const provider = new JsonRpcProvider({ url: RPC });

export function agentAccount(): Account {
  const key = process.env.AGENT_PRIVATE_KEY;
  if (!key) throw new Error("AGENT_PRIVATE_KEY missing: run `npm run keygen`, then add the key on chain (README)");
  return new Account(AGENT, provider, key as KeyPairString);
}

export async function view<T>(method: string, args: Record<string, unknown> = {}, contractId = WARBAND): Promise<T> {
  return (await provider.callFunction({ contractId, method, args })) as T;
}

export interface Market {
  day_index: string;
  price: string;
  floor_price: string;
  buyback_open: boolean;
  buyback_reserve: string;
  dao_pending: string;
}

export interface Holder {
  account_id: string;
  weight: string;
  delegated: string;
  dao_registered: boolean;
}

export function log(event: string, data: Record<string, unknown> = {}) {
  console.log(JSON.stringify({ at: new Date().toISOString(), event, ...data }));
}

/** Short, readable reason from an RPC or execution error. */
export function reason(e: unknown): string {
  const err = e as { type?: string; message?: string; cause?: unknown };
  const text = [err.type, err.message, JSON.stringify(err.cause ?? "")].filter(Boolean).join(" | ");
  return text.replace(/\s+/g, " ").slice(0, 300);
}
