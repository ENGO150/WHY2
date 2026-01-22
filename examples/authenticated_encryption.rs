/*
This is part of WHY2
Copyright (C) 2022-2026 Václav Šmejkal

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU General Public License for more details.

You should have received a copy of the GNU General Public License
along with this program.  If not, see <https://www.gnu.org/licenses/>.
*/

//! Authenticated Encryption Example
//!
//! Demonstrates the "Encrypt-then-MAC" workflow using HMAC-SHA256.
//! This ensures that data cannot be modified in transit without detection.
//!
//! Run with: cargo run --example authenticated_encryption

use why2::
{
    encrypter,
    decrypter,
    crypto,
    auth::AuthenticatedData,
    grid::Grid,
};

fn main() -> Result<(), Box<dyn std::error::Error>>
{
    println!("=== WHY2 Authenticated Encryption (Encrypt-then-MAC) ===\n");

    let message = "Transfer $1,000,000 to Alice.";
    println!("Original: {}", message);

    //STEP 1: GENERATE KEYS
    //IMPORTANT: ENCRYPTION KEY AND MAC MUST BE DIFFERENT!
    let enc_key = crypto::generate_key::<8, 8>();
    let mac_key = [0x42u8; 32]; //IN REAL USAGE, DERIVE THIS SECURELY

    println!("\n[Sender] Encrypting and Signing...");

    //STEP 2: ENCRYPT
    let encrypted = encrypter::encrypt_string::<8, 8>(message, Some(&enc_key))?;

    //STEP 3: AUTHENTICATE (ADD HMAC TAG)
    let auth_package = AuthenticatedData::authenticate(encrypted, &mac_key);

    println!("✓ Data encrypted and signed.");
    println!("  MAC Tag (first 8 bytes): {:x?}", &auth_package.mac[..8]);

    //STEP 4: SERIALIZE (SIMULATE SENDING OVER NETWORK)
    let mut network_packet: Vec<u8> = auth_package.into();
    println!("✓ Serialized into {} bytes for transmission.", network_packet.len());

    println!("\n--- Network Transmission ---");

    //STEP 5: ATTACK SIMULATION (MAN-IN-THE-MIDDLE)
    //ATTEMPT TO TAMPER WITH DATA (FLIP A BIT IN CIPHERTEXT)
    //PACKET STRUCTURE: [MAC (32)][NONCE (64)][CIPHERTEXT...]
    let tamper_index = 32 + 64 + 5;
    network_packet[tamper_index] ^= 0xFF;
    println!("⚠️  ATTACKER: Modified byte at index {}!", tamper_index);

    println!("\n[Receiver] Verifying...");

    //STEP 6: DESERIALIZE
    //TRY TO PARSE RAW BYTES INTO AuthenticatedData
    let received_package = AuthenticatedData::<8, 8>::try_from(network_packet.as_slice())?;

    //STEP 7: VERIFY INTEGRITY
    if received_package.verify(&mac_key)
    {
        println!("❌ VERIFICATION PASSED? This should not happen!");
    } else
    {
        println!("✓ VERIFICATION FAILED! Tampering detected. Discarding data.");
    }

    //STEP 8: RESTORE & VERIFY VALID DATA
    println!("\n[Receiver] Retrying with valid data...");

    //REPAIR THE PACKET
    network_packet[tamper_index] ^= 0xFF;
    let valid_package = AuthenticatedData::<8, 8>::try_from(network_packet.as_slice())?;

    if valid_package.verify(&mac_key)
    {
        println!("✓ Integrity verified. Proceeding to decrypt.");

        //EXTRACT INNER EncryptedData
        let mut inner_data = valid_package.encrypted_data;

        //ADD KEY
        inner_data.key = Grid::from_key(&enc_key)?;

        //DECRYPT
        let decrypted = decrypter::decrypt_string(inner_data)?;

        println!("✓ Decrypted message: {}", *decrypted);
        assert_eq!(message, *decrypted);
    } else
    {
        println!("❌ Integrity check failed on valid data!");
    }

    Ok(())
}
