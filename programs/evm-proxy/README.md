# EVM proxy referral registration

An EVM EOA signs this EIP-712 primary type:

```text
SetReferralCode(bytes32 proxyAccount,string code,uint64 nonce,uint64 deadline)
```

The instruction carries eight code bytes. The program encodes those bytes as base58 and removes leading `1` characters before hashing the signed string, matching Store registration. The zero code is invalid. JavaScript accepts Store base58 aliases, pads decoded bytes to eight bytes, and returns the canonical string in `buildSetReferralCodeTypedData`. Sign that returned payload. For example, `2`, `12` and `11111112` normalize to the same code bytes, but signing those strings verbatim produces different signatures.

Use the Config's Store and genesis hash to build the domain. Rust exposes `SetReferralCodeIntent::typed_data`, `digest`, `set_referral_code_instruction` and `Client::set_proxy_referral_code_transaction` behind the `evm-proxy` feature. JavaScript exports `buildSetReferralCodeTypedData` and `buildSetReferralCodeTransaction` through the existing `js` feature. Pass nonce and deadline as decimal strings in JavaScript to preserve all 64 bits. The transaction builder takes separate `submitter` and `payer` keys; both must sign when they differ.

## Known issues

Every action and allowed EVM chain shares one wrapping `u64` nonce per Proxy. MAX advances to zero. This finite counter does not establish absolute lifetime replay resistance after an entire nonce cycle if a historical signature's other validity conditions recur. Registration does not change that policy.
