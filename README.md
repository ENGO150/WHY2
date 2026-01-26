# WHY2
[![Build Status]][pipelines] [![Codacy Badge]][gitlab] [![Latest Version]][crates.io] [![Docs Status]][docs]

[Build Status]: https://git.satan.red/ENGO150/WHY2/badges/development/pipeline.svg
[pipelines]: https://git.satan.red/ENGO150/WHY2/-/pipelines
[Latest Version]: https://img.shields.io/crates/v/why2
[crates.io]: https://crates.io/crates/why2
[Docs Status]: https://img.shields.io/docsrs/why2
[docs]: https://docs.rs/why2
[Codacy Badge]: https://app.codacy.com/project/badge/Grade/80836146f6fa4567b734e7b5ed452f2d
[gitlab]: https://git.satan.red/ENGO150/WHY2

**Lightweight, fast, secure, and easy to use encryption system.**

---

## Code Example
```rust
use why2::{ encrypter, decrypter };

fn main()
{
    let input = String::from("Hello world!");

    // Encrypt input using 8x8 Grid, with random key
    let encrypted = encrypter::encrypt_string::<8, 8>(&input, None)
                        .expect("Encryption failed");

    // Print encrypted Grids
    for grid in &encrypted.output
    {
        println!("{}", grid);
    }

    // Decrypt
    let decrypted = decrypter::decrypt_string(encrypted)
                        .expect("Decryption failed");

    // Compare input & output
    assert_eq!(input, *decrypted);
}
```

## Links

- [API Documentation](https://docs.rs/why2)
- [Examples Directory](https://git.satan.red/ENGO150/WHY2/-/tree/stable/examples)
- [Security Policy](https://git.satan.red/ENGO150/WHY2/-/blob/stable/SECURITY)
- [Contributing Guidelines](https://git.satan.red/ENGO150/WHY2/-/blob/stable/CONTRIBUTING)

## Getting Help
For help, DM me directly on [Discord](https://discord.com/users/634385503956893737) :)

## Security Notice

WHY2 is an **experimental symmetric encryption algorithm**. While it employs
proven cryptographic techniques (SPN structure, ARX operations, MDS mixing),
it has **not been formally audited or peer-reviewed**.

#### License
<sup>
WHY2 is licensed under the GNU GPLv3. You are free to use, modify, and redistribute it under the terms of the license. See <a href="https://www.gnu.org/licenses/" target="_blank">https://www.gnu.org/licenses/</a> for details.
</sup>
