# EVM proxy

The program executes EVM EOA signatures through a permanent Solana proxy PDA. A proxy is derived from the raw 20-byte EVM address and the selected program ID. Anyone can fund `create_proxy_idempotent`; the payer gains no authority. Repeating creation preserves the existing account, and the program has no close instruction.

## Deployment policy

The proxy program's current upgrade authority creates the singleton Config, supplying its first admin as an account. Config binds one Store and a supplied genesis hash, records its canonical bump, and starts paused with no allowed EVM chains. The program does not independently attest the supplied genesis hash. The Rust initialization client checks the connected RPC's hash before sending and separately reports immutable Store/hash readback after confirmation. Incomplete readback retains the creation signature.

The admin manages the full-width chain allowlist, pause state and next-admin nomination. The nominated account must sign acceptance before gaining authority. Policy changes reject no-ops; replacing a nomination invalidates the earlier nominee. Store and genesis hash remain immutable.

## Signed intents

All intents use the EIP-712 name `GMX Solana EVM Proxy`, version `1`, the signed uint256 chain ID, and a salt equal to keccak256 of the proxy program ID, Config's Store and Config's genesis hash, concatenated as raw 32-byte values.

```text
Cancel(bytes32 proxyAccount,uint64 nonce,uint64 deadline)
Withdraw(bytes32 proxyAccount,string mint,uint64 amount,uint8 decimals,string recipient,uint64 nonce,uint64 deadline)
SetReferralCode(bytes32 proxyAccount,string code,uint64 nonce,uint64 deadline)
```

Signatures are compact 64-byte ECDSA signatures with recovery ID zero or one and nonzero low-s. Execution requires an allowed chain, unpaused Config, the expected next proxy nonce, the Clock deadline window and the recovered EVM owner. Intent instructions derive the proxy's identity and bump from its loaded account; the proxy supplies its EVM address.

- Cancel consumes the expected nonce without transferring assets.
- Withdraw transfers exactly the signed amount through legacy SPL Token TransferChecked. The signed mint and recipient are base58 strings; existing canonical source and destination ATAs are required. Zero amounts and self-recipient withdrawals are rejected.
- SetReferralCode prepares the proxy's Store User and registers the signed code through Store CPIs. Its signer funds only the targets' rent deficits. A zero code, an occupied code target, an invalid User or a failing CPI rejects atomically. The transaction fee remains payable on an executed failed transaction.

Registration carries eight code bytes. The signed string is their base58 encoding with leading `1` characters removed, matching Store. JS accepts aliases, pads decoded bytes to eight bytes, and returns the canonical string in `build_evm_proxy_set_referral_code_typed_data`. Sign that returned payload: `2`, `12` and `11111112` normalize to the same bytes, but signing those strings verbatim produces different signatures.

## SDK

Rust exposes PDA helpers, intent typed-data/digest types, ProgramExt instruction constructors, builders implementing IntoAtomicGroup, and EvmProxyOps behind the `evm-proxy` feature. Operations select the proxy deployment explicitly. The `js` feature exposes snake_case derivation, signing-payload and transaction-group functions with named argument objects. Transaction builders accept the normal group and priority-fee options, including the shared SDK minimum priority fee unless overridden. Referral registration takes separate `submitter` and `payer` keys; both sign when they differ.

Pass nonce, deadline and token amount as decimal strings in JS to preserve all 64 bits. The signed EIP-712 keys retain their canonical spellings, including `chainId`, `proxyAccount` and `primaryType`.

Successful intents emit IntentConsumed; Withdraw and referral registration also emit their effect events.

## Nonce and event contract

Each proxy stores its next expected nonce, initially 0, shared across all intents and allowed EVM chains. Successful execution stores the checked successor. A matching u64::MAX fails with NonceExhausted and no business-state change; the counter never wraps.

The execution window is inclusive: now <= deadline <= now + 3600 seconds, using Solana's Clock. InvalidDeadline covers invalid timestamps and deadlines outside this window.

Config changes and successful intents emit CPI events. Every emitting instruction and SDK/JS output includes this Proxy deployment's event authority and program accounts. With the decode feature, gmsol-sdk re-exports the canonical ProxyCPIEvent and decode_committed_events API from gmsol-decode. Consume successful transaction metadata for the selected deployment; failed inner traces do not establish committed changes, and malformed recognized events return errors.
