import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { bytesToHex, hashTypedData, hexToBytes, numberToBytes } from "viem";
import { generatePrivateKey, privateKeyToAccount } from "viem/accounts";
import sdk from "../../pkg-node/gmsol_sdk.js";

const { build_evm_proxy_cancel_transaction, build_evm_proxy_cancel_typed_data } = sdk;

const EVM_RECOVERY_ID_OFFSET = 27;
const SYSTEM_PROGRAM = "11111111111111111111111111111111";

const vector = JSON.parse(
  readFileSync(new URL("../../../evm-proxy-intent/fixtures/cancel.json", import.meta.url)),
);
const cancel = {
  program: { id: vector.program },
  evm_address: vector.evmAddress,
  chain_id: BigInt(vector.chainId),
  nonce: BigInt(vector.nonce),
  deadline: BigInt(vector.deadline),
};
const typedData = () =>
  build_evm_proxy_cancel_typed_data({
    ...cancel,
    store: vector.store,
    genesis_hash: vector.genesisHash,
  });

test("Cancel typed data hashes to the digest the program verifies", () => {
  assert.equal(hashTypedData(typedData()), vector.digest);
});

test("Cancel transaction carries the signed fields and a viem signature", async () => {
  const signature = hexToBytes(
    await privateKeyToAccount(generatePrivateKey()).signTypedData(typedData()),
  );
  const group = build_evm_proxy_cancel_transaction({
    ...cancel,
    signature: bytesToHex(signature),
    payer: SYSTEM_PROGRAM,
    recent_blockhash: SYSTEM_PROGRAM,
  });
  const [[transaction]] = group.serialize();
  group.free();
  const u64 = (value) => numberToBytes(value, { size: 8 }).reverse();
  const args = Buffer.concat([
    numberToBytes(cancel.chain_id, { size: 32 }),
    u64(cancel.nonce),
    u64(cancel.deadline),
    signature.subarray(0, 64),
    Uint8Array.of(signature[64] - EVM_RECOVERY_ID_OFFSET),
  ]);
  assert.ok(Buffer.from(transaction).includes(args));
});
