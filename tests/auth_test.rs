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
    auth::AuthenticatedData,
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
