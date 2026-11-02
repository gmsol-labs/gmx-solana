// Build a signed EVM intent and an unsigned Solana transaction with the public JS SDK.
const assert = require('node:assert/strict');
const path = require('node:path');
const { hashTypedData, recoverTypedDataAddress } = require('viem');
const { generatePrivateKey, privateKeyToAccount } = require('viem/accounts');
const sdk = require(path.resolve(__dirname, '../../../crates/sdk/pkg'));

async function main() {
  const chunks = [];
  for await (const chunk of process.stdin) chunks.push(chunk);
  const input = JSON.parse(Buffer.concat(chunks).toString('utf8'));
  const signer = privateKeyToAccount(generatePrivateKey());
  const args = { ...input.intent, evmAddress: signer.address };
  const typed = sdk.buildWithdrawTypedData(args);
  const signature = await signer.signTypedData(typed);
  assert.equal(await recoverTypedDataAddress({ ...typed, signature }), signer.address);
  const v = Number.parseInt(signature.slice(-2), 16);
  const recoveryId = v >= 27 ? v - 27 : v;
  assert.ok(recoveryId === 0 || recoveryId === 1);
  const group = sdk.buildWithdrawTransaction(
    args, input.payer, input.blockhash, signature.slice(0, -2), recoveryId,
  );
  const batches = group.serialize();
  assert.equal(batches.length, 1);
  assert.equal(batches[0].length, 1);
  process.stdout.write(JSON.stringify({
    evmAddress: signer.address,
    digest: hashTypedData(typed),
    transaction: Buffer.from(batches[0][0]).toString('base64'),
  }));
}

main().catch(error => { console.error(error); process.exitCode = 1; });
