const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { concatHex, hashDomain, hashStruct, hashTypedData, keccak256, recoverAddress, stringToHex } = require('viem');
const { generatePrivateKey, privateKeyToAccount } = require('viem/accounts');
const { PublicKey } = require('@solana/web3.js');

assert.equal(require('viem/package.json').version, '2.57.7');
assert.equal(require('@solana/web3.js/package.json').version, '1.98.4');

const programAddress = 'Dodg22E8BxfCtYBzHVHmAzFqD5bZy6q9Tp4NdYSxkmp6';
const programKey = new PublicKey(programAddress);
const alphabet = '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz';
let programNumber = 0n;
for (const char of programAddress) {
  const digit = alphabet.indexOf(char);
  assert(digit >= 0);
  programNumber = programNumber * 58n + BigInt(digit);
}
const program = `0x${programNumber.toString(16).padStart(64, '0')}`;
const bytes = (byte) => `0x${byte.repeat(32)}`;
const word = (value) => `0x${value.toString(16).padStart(64, '0')}`;
const now = 1_700_000_000n;
const futureNow = now + 7_200n;
const types = {
  Cancel: [
    { name: 'proxyAccount', type: 'bytes32' },
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
const typeHash = keccak256(stringToHex('Cancel(bytes32 proxyAccount,uint64 nonce,uint64 deadline)'));
const oldPairTypes = {
  Cancel: [types.Cancel[0], { name: 'generation', type: 'uint64' }, ...types.Cancel.slice(1)],
};
const base = {
  program,
  store: bytes('22'),
  genesis: bytes('42'),
  chainId: 1n,
  domainName: 'GMX Solana EVM Proxy',
  version: '1',
  nonce: 1n,
  deadline: now + 120n,
};
const cases = [
  ['first', {}],
  ['ordinary', { nonce: 2n }],
  ['wrong-nonce', { nonce: 3n }],
  ['near-wrap-cancel', { nonce: (1n << 64n) - 2n }],
  ['near-wrap-competing', { nonce: (1n << 64n) - 2n, deadline: futureNow + 120n }],
  ['max', { nonce: (1n << 64n) - 1n, deadline: futureNow + 120n }],
  ['wrong-zero', { nonce: 0n }],
  ['zero', { nonce: 0n, deadline: futureNow + 120n }],
  ['future-presigned', { nonce: 1n, deadline: futureNow + 120n }],
  ['large-nonce', { nonce: (1n << 53n) + 1n }],
  ['deadline-now', { deadline: now }],
  ['deadline-max', { deadline: now + 3_600n }],
  ['deadline-past', { deadline: now - 1n }],
  ['deadline-too-far', { deadline: now + 3_601n }],
  ['other-chain', { chainId: 2n }],
  ['full-width-chain', { chainId: (1n << 255n) + 42_161n }],
  ['other-program', { program: bytes('11') }],
  ['other-store', { store: bytes('23') }],
  ['other-genesis', { genesis: bytes('43') }],
  ['other-name', { domainName: 'GMX Solana EVM Proxy V2' }],
  ['other-version', { version: '2' }],
  ['other-proxy', { proxyAccount: bytes('44') }],
];

function canonicalProxy(address) {
  const [key] = PublicKey.findProgramAddressSync(
    [Buffer.from('proxy'), Buffer.from(address.slice(2), 'hex')],
    programKey,
  );
  return `0x${Buffer.from(key.toBytes()).toString('hex')}`;
}

async function vectorsFor(address, account) {
  const proxyAccount = canonicalProxy(address);
  const vectors = [];
  for (const [name, changes] of cases) {
    const value = { ...base, proxyAccount, ...changes };
    const domain = {
      name: value.domainName,
      version: value.version,
      chainId: value.chainId,
      salt: keccak256(concatHex([value.program, value.store, value.genesis])),
    };
    const message = {
      proxyAccount: value.proxyAccount,
      nonce: value.nonce,
      deadline: value.deadline,
    };
    const vector = {
      name,
      typeHash,
      program: value.program,
      store: value.store,
      genesis: value.genesis,
      chainId: word(value.chainId),
      domainName: value.domainName,
      version: value.version,
      proxyAccount: value.proxyAccount,
      nonce: value.nonce.toString(),
      deadline: value.deadline.toString(),
      domainSeparator: hashDomain({ domain, types: domainTypes }),
      structHash: hashStruct({ data: message, primaryType: 'Cancel', types }),
      digest: hashTypedData({ domain, message, primaryType: 'Cancel', types }),
    };
    if (name === 'first') {
      const oldMessage = { ...message, generation: 0n };
      vector.oldPairStructHash = hashStruct({ data: oldMessage, primaryType: 'Cancel', types: oldPairTypes });
      vector.oldPairDigest = hashTypedData({ domain, message: oldMessage, primaryType: 'Cancel', types: oldPairTypes });
      assert.notEqual(vector.oldPairDigest, vector.digest);
      if (account) {
        const oldSignature = await account.signTypedData({ domain, message: oldMessage, primaryType: 'Cancel', types: oldPairTypes });
        vector.oldPairSignature = oldSignature.slice(0, -2);
        vector.oldPairRecoveryId = Number.parseInt(oldSignature.slice(-2), 16) - 27;
      }
    }
    if (account) {
      const signature = await account.signTypedData({ domain, message, primaryType: 'Cancel', types });
      const v = Number.parseInt(signature.slice(-2), 16);
      vector.signature = signature.slice(0, -2);
      vector.recoveryId = v >= 27 ? v - 27 : v;
      assert(vector.recoveryId === 0 || vector.recoveryId === 1);
    }
    vectors.push(vector);
  }
  return { generator: 'viem@2.57.7', pdaLibrary: '@solana/web3.js@1.98.4', programAddress,
    evmAddress: address, canonicalProxyAccount: proxyAccount, now: now.toString(),
    futureNow: futureNow.toString(), vectors };
}

async function main() {
  const destination = path.join(__dirname, '..', 'cancel-pair-vectors.json');
  if (process.argv[2] === '--write') {
    const account = privateKeyToAccount(generatePrivateKey());
    const output = await vectorsFor(account.address.toLowerCase(), account);
    fs.writeFileSync(destination, `${JSON.stringify(output, null, 2)}\n`);
  } else {
    assert.equal(process.argv[2], '--check');
    const saved = JSON.parse(fs.readFileSync(destination, 'utf8'));
    const expected = await vectorsFor(saved.evmAddress, null);
    assert.equal(saved.generator, expected.generator);
    assert.equal(saved.pdaLibrary, expected.pdaLibrary);
    assert.equal(saved.canonicalProxyAccount, expected.canonicalProxyAccount);
    assert.equal(saved.vectors.length, expected.vectors.length);
    for (let i = 0; i < expected.vectors.length; i += 1) {
      const actual = saved.vectors[i];
      for (const [key, value] of Object.entries(expected.vectors[i])) {
        assert.deepEqual(actual[key], value, `${actual.name}.${key}`);
      }
      const v = (actual.recoveryId + 27).toString(16).padStart(2, '0');
      const recovered = await recoverAddress({ hash: actual.digest, signature: `${actual.signature}${v}` });
      assert.equal(recovered.toLowerCase(), saved.evmAddress);
      if (actual.name === 'first') {
        const oldV = (actual.oldPairRecoveryId + 27).toString(16).padStart(2, '0');
        const signature = `${actual.oldPairSignature}${oldV}`;
        const oldRecovered = await recoverAddress({ hash: actual.oldPairDigest, signature });
        assert.equal(oldRecovered.toLowerCase(), saved.evmAddress);
        const newRecovered = await recoverAddress({ hash: actual.digest, signature });
        assert.notEqual(newRecovered.toLowerCase(), saved.evmAddress);
      }
    }
  }
  console.log(`Verified ${cases.length} single-nonce Cancel vectors with viem 2.57.7`);
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
