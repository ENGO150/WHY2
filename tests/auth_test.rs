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

use why2::
{
    encrypter,
    decrypter,
    auth::{ AuthenticatedData, AuthenticatedPacket },
};

#[test]
fn test_auth_tamper_resistance()
{
    let message = "Time was invented by clock companies to sell more clocks. I know Marx didn't\
        say that, but I find it really funny, if you do not use it to mock him.";
    let mac_key = [42u8; 32];

    //ENCRYPT
    let encrypted = encrypter::encrypt_string::<8, 8>(message, None)
        .expect("Encryption failed");

    //AUTH
    let mut auth_data = AuthenticatedData::authenticate(encrypted, &mac_key);

    //INITIAL VERIFICATION (SHOULD WORK)
    assert!(auth_data.verify(&mac_key), "Verifying valid data failed!");

    //FLIP BITS
    let original_val = auth_data.encrypted_data.output[0][0][0];
    auth_data.encrypted_data.output[0][0][0] ^= 1;

    assert!(!auth_data.verify(&mac_key), "Verifying invalid data succeeded!");

    //RESTORE ORIGINAL TEXT
    auth_data.encrypted_data.output[0][0][0] = original_val;

    //ATTACK NONCE
    auth_data.encrypted_data.nonce[0][0] ^= 1;
    assert!(!auth_data.verify(&mac_key), "Verifying invalid data (nonce) succeeded!");
    auth_data.encrypted_data.nonce[0][0] ^= 1; //RESTORE NONCE

    //ATTACK MAC TAG
    auth_data.mac[0] = auth_data.mac[0].wrapping_add(1);
    assert!(!auth_data.verify(&mac_key), "Verifying invalid data (MAC tag) succeeded!");
}

#[test]
fn test_authenticated_packet_roundtrip()
{
    let message = "authenticated packet\0with NUL";
    let mac_key = [7u8; 32];

    let encrypted = encrypter::encrypt_string::<8, 8>(message, None)
        .expect("Encryption failed");
    let key = encrypted.key.clone();

    let auth_data = AuthenticatedData::authenticate(encrypted, &mac_key);
    let bytes: Vec<u8> = auth_data.into();

    //THE KEY MUST NOT BE PART OF THE SERIALIZED PACKET
    let key_bytes: Vec<u8> = key.iter()
        .flat_map(|row| row.iter())
        .flat_map(|value| value.to_be_bytes())
        .collect();
    assert!(
        !bytes.windows(key_bytes.len()).any(|window| window == key_bytes.as_slice()),
        "serialized packet leaked the key"
    );

    let packet = AuthenticatedPacket::<8, 8>::try_from(bytes.as_slice())
        .expect("Packet parsing failed");
    assert!(packet.verify(&mac_key));

    //A WRONG MAC MUST NOT OPEN THE PACKET
    assert!(packet.clone().open(&[0u8; 32], key.clone()).is_none());

    //THE CORRECT MAC ATTACHES THE SEPARATELY STORED KEY
    let encrypted = packet.open(&mac_key, key).expect("Packet did not open");
    let decrypted = decrypter::decrypt_string(encrypted).expect("Decryption failed");

    assert_eq!(message, &*decrypted);
}

#[test]
fn test_authenticated_packet_rejects_tampering()
{
    let message = "tamper detection";
    let mac_key = [9u8; 32];

    let encrypted = encrypter::encrypt_string::<8, 8>(message, None)
        .expect("Encryption failed");
    let auth_data = AuthenticatedData::authenticate(encrypted, &mac_key);
    let bytes: Vec<u8> = auth_data.into();

    //CIPHERTEXT STARTS AFTER THE 32-BYTE MAC AND ONE 8x8 NONCE GRID
    let ciphertext_start = 32 + 8 * 8 * 8;
    let mut tampered_ciphertext = bytes.clone();
    tampered_ciphertext[ciphertext_start] ^= 1;
    let packet = AuthenticatedPacket::<8, 8>::try_from(tampered_ciphertext.as_slice())
        .expect("Packet parsing failed");
    assert!(!packet.verify(&mac_key), "tampered ciphertext must fail");

    //TAMPERING THE NONCE MUST ALSO FAIL
    let mut tampered_nonce = bytes.clone();
    tampered_nonce[32] ^= 1;
    let packet = AuthenticatedPacket::<8, 8>::try_from(tampered_nonce.as_slice())
        .expect("Packet parsing failed");
    assert!(!packet.verify(&mac_key), "tampered nonce must fail");

    //REPLAY IS NOT PREVENTED BY THE PACKET API; THE DOCUMENTATION SAYS SO
    let replayed = AuthenticatedPacket::<8, 8>::try_from(bytes.as_slice())
        .expect("Packet parsing failed");
    assert!(replayed.verify(&mac_key), "replayed packet must still verify");
}
