# WHY2

REX, a configurable grid-based block cipher in CTR mode, with optional HMAC-SHA256 authentication.

[![build](https://img.shields.io/gitlab/pipeline-status/ENGO150/WHY2?branch=stable&style=flat-square&gitlab_url=https%3A%2F%2Fgit.satan.red)](https://git.satan.red/ENGO150/WHY2/-/pipelines)
[![why2](https://img.shields.io/crates/v/why2?label=why2&style=flat-square)](https://crates.io/crates/why2)
[![docs](https://img.shields.io/docsrs/why2?style=flat-square)](https://docs.rs/why2)
[![license](https://img.shields.io/badge/license-GPLv3-blue?style=flat-square)](https://git.satan.red/ENGO150/WHY2/-/blob/stable/LICENSE)

[Highlights](#highlights) · [Install](#install) · [Usage](#usage) · [Cargo features](#cargo-features) · [Security](#security) · [Links](#links)

## Highlights

- Configurable grid dimensions, with 8×8 (4096-bit) blocks by default.
- Keyed ARX mixing and MDS diffusion over GF(2^64).
- CTR mode with one-shot encryption and decryption.
- `RexStream` for incremental chunks.
- Encrypt-then-MAC with HMAC-SHA256.
- Constant-time operations and zeroization.

## Install

```bash
cargo add why2
```

## Usage

```rust
use why2::{ encrypter, decrypter };

fn main()
{
    let input = String::from("Hello world!");
    let encrypted = encrypter::encrypt_string::<8, 8>(&input, None)
        .expect("encryption failed");

    for grid in &encrypted.output
    {
        println!("{}", grid);
    }

    let decrypted = decrypter::decrypt_string(encrypted)
        .expect("decryption failed");

    assert_eq!(input, *decrypted);
}
```

`None` generates a random key; pass `Some(&key)` to supply one. `EncryptedData` also carries the
nonce and the key used. Do not serialize the key with the ciphertext.

## Cargo features

| Feature | Default | Description |
|:---|:---|:---|
| `constant-time` | yes | Constant-time conditionals through `subtle`; disabling opens timing side channels |
| `auth` | yes | Encrypt-then-MAC with HMAC-SHA256 through `AuthenticatedData` |

## Security

- REX is experimental and has not been formally audited.
- Encryption alone does not detect tampering; enable `auth` for authenticated data.
- `RexStream` needs a nonce range disjoint from every other stream under the same key.

## Links

- [Documentation](https://docs.rs/why2)
- [Examples](https://git.satan.red/ENGO150/WHY2/-/tree/stable/examples)
- [WHY2 Chat](https://git.satan.red/ENGO150/WHY2/-/blob/stable/chat/README.md), the reference application
- [Security policy](https://git.satan.red/ENGO150/WHY2/-/blob/stable/SECURITY)

## License

GPLv3. See [LICENSE](https://git.satan.red/ENGO150/WHY2/-/blob/stable/LICENSE).
