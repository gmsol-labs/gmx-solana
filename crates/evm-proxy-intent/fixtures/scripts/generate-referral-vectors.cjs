const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { concatHex, keccak256, hashStruct, hashTypedData } = require('viem');
const { base58 } = require('@scure/base');
const { PublicKey } = require('@solana/web3.js');

assert.equal(require('viem/package.json').version, '2.57.7');
assert.equal(require('@solana/web3.js/package.json').version, '1.98.4');
const destination = path.join(__dirname, '..', 'referral-vectors.json');
// Deployment/domain inputs are public fixture parameters; all expected hashes are regenerated.
const fixture = JSON.parse(fs.readFileSync(destination, 'utf8'));
const { vectors: previous, ...parameters } = fixture;
const [proxy] = PublicKey.findProgramAddressSync([
  Buffer.from('proxy'), Buffer.from(fixture.evmAddress.slice(2), 'hex'),
], new PublicKey(Buffer.from(fixture.program.slice(2), 'hex')));
assert.equal(`0x${proxy.toBuffer().toString('hex')}`, fixture.proxy);
const salt = keccak256(concatHex([fixture.program, fixture.store, fixture.genesis]));
assert.equal(salt, fixture.salt);
const codes = ['0000000000000000', '0000000000000001', '00000000000000ff',
  '0001020304050607', '8000000000000000', 'ffffffffffffffff'];
const nonces = ['0', '1', '9007199254740993', '18446744073709551615'];
const deadline = '18446744073709551615';
const chainId = previous[0].chainId;
const vectors = codes.flatMap(bytesHex => nonces.map(nonce => {
  const canonical = base58.encode(Buffer.from(bytesHex, 'hex')).replace(/^1+/, '');
  const domain = { name: 'GMX Solana EVM Proxy', version: '1', chainId: BigInt(chainId), salt };
  const message = { proxyAccount: fixture.proxy, code: canonical, nonce: BigInt(nonce), deadline: BigInt(deadline) };
  return { bytesHex, canonical, nonce, deadline, chainId,
    structHash: hashStruct({ data: message, primaryType: 'SetReferralCode', types: fixture.types }),
    digest: hashTypedData({ domain, message, primaryType: 'SetReferralCode', types: fixture.types }) };
}));
const output = { ...parameters, vectors };
if (process.argv[2] === '--write') fs.writeFileSync(destination, `${JSON.stringify(output, null, 2)}\n`);
else { assert.equal(process.argv[2], '--check'); assert.deepEqual(fixture, output); }
console.log(`Verified ${vectors.length} referral vectors with viem 2.57.7`);
