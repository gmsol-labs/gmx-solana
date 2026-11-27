const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { concatHex, hashDomain, hashStruct, hashTypedData, keccak256 } = require('viem');
const { PublicKey } = require('@solana/web3.js');

assert.equal(require('viem/package.json').version, '2.57.7');
assert.equal(require('@solana/web3.js/package.json').version, '1.98.4');

const type = 'Withdraw(bytes32 proxyAccount,string mint,uint64 amount,uint8 decimals,string recipient,uint64 nonce,uint64 deadline)';
const types = {
  Withdraw: [
    { name: 'proxyAccount', type: 'bytes32' },
    { name: 'mint', type: 'string' },
    { name: 'amount', type: 'uint64' },
    { name: 'decimals', type: 'uint8' },
    { name: 'recipient', type: 'string' },
    { name: 'nonce', type: 'uint64' },
    { name: 'deadline', type: 'uint64' },
  ],
};
const domainTypes = {
  EIP712Domain: [
    { name: 'name', type: 'string' },
    { name: 'version', type: 'string' },
    { name: 'chainId', type: 'uint256' },
    { name: 'salt', type: 'bytes32' },
  ],
};
const bytes = (byte) => `0x${byte.repeat(32)}`;
const word = (value) => `0x${value.toString(16).padStart(64, '0')}`;
const programKey = new PublicKey('Dodg22E8BxfCtYBzHVHmAzFqD5bZy6q9Tp4NdYSxkmp6');
const evmAddress = `0x${'11'.repeat(20)}`;
const [proxy] = PublicKey.findProgramAddressSync(
  [Buffer.from('proxy'), Buffer.from(evmAddress.slice(2), 'hex')], programKey,
);
const base = {
  program: `0x${programKey.toBuffer().toString('hex')}`, evmAddress,
  store: bytes('22'), genesis: bytes('42'), chainId: 1n,
  proxy: `0x${proxy.toBuffer().toString('hex')}`, mint: bytes('07'), amount: 125n,
  decimals: 6, recipient: bytes('09'), nonce: 1n, deadline: 1_700_000_120n,
};
const cases = [
  ['first', {}], ['different-mint', { mint: bytes('08') }],
  ['different-amount', { amount: 126n }], ['different-decimals', { decimals: 7 }],
  ['different-recipient', { recipient: bytes('0a') }],
  ['large-nonce', { nonce: 9_007_199_254_740_993n }], ['different-nonce', { nonce: 2n }],
  ['different-deadline', { deadline: 1_700_000_121n }],
  ['different-chain', { chainId: (1n << 64n) + 1n }],
  ['different-store', { store: bytes('23') }], ['zero', { nonce: 0n }],
  ['max', { nonce: (1n << 64n) - 1n }],
];
const vectors = cases.map(([name, overrides]) => {
  const value = { ...base, ...overrides };
  const mint = new PublicKey(Buffer.from(value.mint.slice(2), 'hex')).toBase58();
  const recipient = new PublicKey(Buffer.from(value.recipient.slice(2), 'hex')).toBase58();
  const salt = keccak256(concatHex([value.program, value.store, value.genesis]));
  const domain = { name: 'GMX Solana EVM Proxy', version: '1', chainId: value.chainId, salt };
  const message = { proxyAccount: value.proxy, mint, amount: value.amount,
    decimals: value.decimals, recipient, nonce: value.nonce, deadline: value.deadline };
  return { name, program: value.program, evmAddress, store: value.store,
    genesis: value.genesis, chainId: word(value.chainId), proxy: value.proxy,
    mint: value.mint, mintBase58: mint, amount: value.amount.toString(), decimals: value.decimals,
    recipient: value.recipient, recipientBase58: recipient, nonce: value.nonce.toString(),
    deadline: value.deadline.toString(), salt,
    domainSeparator: hashDomain({ domain, types: domainTypes }),
    structHash: hashStruct({ data: message, primaryType: 'Withdraw', types }),
    digest: hashTypedData({ domain, message, primaryType: 'Withdraw', types }) };
});
const output = { generator: 'viem@2.57.7', type, vectors };
const destination = path.join(__dirname, '..', 'withdraw-vectors.json');
if (process.argv[2] === '--write') fs.writeFileSync(destination, `${JSON.stringify(output, null, 2)}\n`);
else {
  assert.equal(process.argv[2], '--check');
  assert.deepEqual(JSON.parse(fs.readFileSync(destination, 'utf8')), output);
}
console.log(`Verified ${cases.length} Withdraw vectors with viem 2.57.7`);
