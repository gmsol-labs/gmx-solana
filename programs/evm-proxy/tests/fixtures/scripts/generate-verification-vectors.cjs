const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { concatHex, hashStruct, hashTypedData, keccak256 } = require('viem');
assert.equal(require('viem/package.json').version, '2.57.7');

const programAddress = 'Dodg22E8BxfCtYBzHVHmAzFqD5bZy6q9Tp4NdYSxkmp6';
const alphabet = '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz';
let programNumber = 0n;
for (const char of programAddress) {
  const digit = alphabet.indexOf(char);
  assert(digit >= 0);
  programNumber = programNumber * 58n + BigInt(digit);
}
const program = `0x${programNumber.toString(16).padStart(64, '0')}`;
const bytes = (byte) => `0x${byte.repeat(32)}`;
const types = { VerificationProbe: [{ name: 'request', type: 'bytes32' }] };
const base = {
  program, store: bytes('22'), genesis: bytes('42'), chainId: 1n,
  name: 'GMX Solana EVM Proxy', version: '1', request: bytes('31'),
};
const cases = [
  ['base', {}, true],
  ['full-width-chain', { chainId: (1n << 255n) + 42161n }, true],
  ['other-chain', { chainId: 2n }, false],
  ['other-name', { name: 'GMX Solana EVM Proxy V2' }, false],
  ['other-version', { version: '2' }, false],
  ['other-program', { program: bytes('11') }, false],
  ['other-store', { store: bytes('23') }, false],
  ['other-genesis', { genesis: bytes('43') }, false],
  ['other-request', { request: bytes('32') }, true],
];
const vectors = cases.map(([name, changes, accepts]) => {
  const value = { ...base, ...changes };
  const domain = {
    name: value.name, version: value.version, chainId: value.chainId,
    salt: keccak256(concatHex([value.program, value.store, value.genesis])),
  };
  const message = { request: value.request };
  return {
    name, accepts, program: value.program, store: value.store, genesis: value.genesis,
    chainId: `0x${value.chainId.toString(16).padStart(64, '0')}`,
    domainName: value.name, version: value.version, request: value.request,
    structHash: hashStruct({ data: message, primaryType: 'VerificationProbe', types }),
    digest: hashTypedData({ domain, message, primaryType: 'VerificationProbe', types }),
  };
});
const output = `${JSON.stringify({ generator: 'viem@2.57.7', programAddress, vectors }, null, 2)}\n`;
const destination = path.join(__dirname, '..', 'verification-vectors.json');
if (process.argv[2] === '--write') fs.writeFileSync(destination, output);
else {
  assert.equal(process.argv[2], '--check');
  assert.equal(fs.readFileSync(destination, 'utf8'), output);
}
console.log(`Verified ${vectors.length} EIP-712 vectors with viem 2.57.7`);
