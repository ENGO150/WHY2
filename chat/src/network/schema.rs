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

//! # Key Exchange Wire Formats
//!
//! `wincode` schemas for the key exchange messages, which carry types from `p521` and `ml-kem`
//! that have no `wincode` encoding of their own.
//!
//! Every field has a fixed size — SEC1 uncompressed points, fixed-width `r || s` signatures
//! (never DER), raw ML-KEM keys and ciphertexts — so an offer and a reply always encode to the
//! same length, and a value that does not parse as what it claims to be (a point off the curve,
//! a malformed key) fails at decode time with [`ReadError::InvalidValue`].

use core::mem::MaybeUninit;

use wincode::
{
    SchemaRead,
    SchemaWrite,
    config::ConfigCore,
    io::{ Reader, Writer },
    error::
    {
        ReadError,
        ReadResult,
        WriteResult,
    },
};

use p521::
{
    PublicKey,
    ecdsa::Signature,
    elliptic_curve::sec1::ToSec1Point,
};

use ml_kem::
{
    MlKem768,
    Ciphertext,
    EncapsulationKey768,
    KeyExport,
    TryKeyInit,
};

use crate::consts;

//STRUCTS
/// The server's half of a key exchange, carried by
/// [`PacketCode::KeyExchangeOffer`](crate::network::codes::PacketCode::KeyExchangeOffer).
#[derive(Clone)]
pub struct Offer //THE SERVER'S HALF OF THE EXCHANGE
{
    /// The server's static identity key.
    pub static_ecc: PublicKey,
    /// The ephemeral ECDH key of this exchange.
    pub eph_ecc: PublicKey,
    /// The ephemeral ML-KEM-768 encapsulation key of this exchange.
    pub pq: EncapsulationKey768,
    /// Signature by `static_ecc` over the transcript (see [`crate::crypto::kex`]).
    pub sig: Signature,
}

/// The client's half of a key exchange, carried by
/// [`PacketCode::KeyExchangeReply`](crate::network::codes::PacketCode::KeyExchangeReply).
///
/// It is unsigned: the client is authenticated afterwards, by its password.
#[derive(Clone)]
pub struct Reply //THE CLIENT'S HALF (UNSIGNED)
{
    /// The client's ephemeral ECDH key.
    pub eph_ecc: PublicKey,
    /// The ML-KEM-768 ciphertext encapsulated to the server's key.
    pub pq: Ciphertext<MlKem768>,
}

//FIELD ADAPTERS
/// Schema for a P-521 public key as a SEC1 uncompressed point.
struct Sec1Key;      //NIST P-521 PUBLIC KEY, SEC1 UNCOMPRESSED
/// Schema for an ECDSA/P-521 signature as fixed-width `r || s`.
struct EcdsaSig;     //ECDSA/P-521 SIGNATURE, FIXED-SIZE (r || s), NEVER DER
/// Schema for an ML-KEM-768 encapsulation key.
struct PqKey;        //ML-KEM-768 ENCAPSULATION KEY
/// Schema for an ML-KEM-768 ciphertext.
struct PqCiphertext; //ML-KEM-768 CIPHERTEXT

//PACKET ADAPTERS
/// Schema for a boxed [`Offer`], used as `#[wincode(with = "BoxedOffer")]`.
pub struct BoxedOffer;
/// Schema for a boxed [`Reply`], used as `#[wincode(with = "BoxedReply")]`.
pub struct BoxedReply;

//IMPLEMENTATIONS
unsafe impl<C: ConfigCore> SchemaWrite<C> for Sec1Key
{
    type Src = PublicKey;

    fn size_of(_: &Self::Src) -> WriteResult<usize> { Ok(consts::ECC_PUBKEY_SIZE) }

    fn write(writer: impl Writer, src: &Self::Src) -> WriteResult<()>
    {
        let point = src.to_sec1_point(false);
        let bytes: &[u8; consts::ECC_PUBKEY_SIZE] = point.as_bytes().try_into()
            .expect("Unexpected SEC1 point length");

        <[u8; consts::ECC_PUBKEY_SIZE] as SchemaWrite<C>>::write(writer, bytes)
    }
}

unsafe impl<'de, C: ConfigCore> SchemaRead<'de, C> for Sec1Key
{
    type Dst = PublicKey;

    fn read(reader: impl Reader<'de>, dst: &mut MaybeUninit<Self::Dst>) -> ReadResult<()>
    {
        let bytes = <[u8; consts::ECC_PUBKEY_SIZE] as SchemaRead<'de, C>>::get(reader)?;

        dst.write(PublicKey::from_sec1_bytes(&bytes)
            .map_err(|_| ReadError::InvalidValue("not a point on P-521"))?);

        Ok(())
    }
}

unsafe impl<C: ConfigCore> SchemaWrite<C> for EcdsaSig
{
    type Src = Signature;

    fn size_of(_: &Self::Src) -> WriteResult<usize> { Ok(consts::ECC_SIGNATURE_SIZE) }

    fn write(writer: impl Writer, src: &Self::Src) -> WriteResult<()>
    {
        let signature = src.to_bytes();
        let bytes: &[u8; consts::ECC_SIGNATURE_SIZE] = signature.as_slice().try_into()
            .expect("Unexpected signature length");

        <[u8; consts::ECC_SIGNATURE_SIZE] as SchemaWrite<C>>::write(writer, bytes)
    }
}

unsafe impl<'de, C: ConfigCore> SchemaRead<'de, C> for EcdsaSig
{
    type Dst = Signature;

    fn read(reader: impl Reader<'de>, dst: &mut MaybeUninit<Self::Dst>) -> ReadResult<()>
    {
        let bytes = <[u8; consts::ECC_SIGNATURE_SIZE] as SchemaRead<'de, C>>::get(reader)?;

        dst.write(Signature::from_slice(&bytes)
            .map_err(|_| ReadError::InvalidValue("not an ECDSA/P-521 signature"))?);

        Ok(())
    }
}

unsafe impl<C: ConfigCore> SchemaWrite<C> for PqKey
{
    type Src = EncapsulationKey768;

    fn size_of(_: &Self::Src) -> WriteResult<usize> { Ok(consts::PQ_PUBKEY_SIZE) }

    fn write(writer: impl Writer, src: &Self::Src) -> WriteResult<()>
    {
        let key = src.to_bytes();
        let bytes: &[u8; consts::PQ_PUBKEY_SIZE] = key.as_slice().try_into()
            .expect("Unexpected ML-KEM encapsulation key length");

        <[u8; consts::PQ_PUBKEY_SIZE] as SchemaWrite<C>>::write(writer, bytes)
    }
}

unsafe impl<'de, C: ConfigCore> SchemaRead<'de, C> for PqKey
{
    type Dst = EncapsulationKey768;

    fn read(reader: impl Reader<'de>, dst: &mut MaybeUninit<Self::Dst>) -> ReadResult<()>
    {
        let bytes = <[u8; consts::PQ_PUBKEY_SIZE] as SchemaRead<'de, C>>::get(reader)?;

        dst.write(EncapsulationKey768::new_from_slice(&bytes)
            .map_err(|_| ReadError::InvalidValue("not an ML-KEM-768 encapsulation key"))?);

        Ok(())
    }
}

unsafe impl<C: ConfigCore> SchemaWrite<C> for PqCiphertext
{
    type Src = Ciphertext<MlKem768>;

    fn size_of(_: &Self::Src) -> WriteResult<usize> { Ok(consts::PQ_CIPHERTEXT_SIZE) }

    fn write(writer: impl Writer, src: &Self::Src) -> WriteResult<()>
    {
        let bytes: &[u8; consts::PQ_CIPHERTEXT_SIZE] = src.as_slice().try_into()
            .expect("Unexpected ML-KEM ciphertext length");

        <[u8; consts::PQ_CIPHERTEXT_SIZE] as SchemaWrite<C>>::write(writer, bytes)
    }
}

unsafe impl<'de, C: ConfigCore> SchemaRead<'de, C> for PqCiphertext
{
    type Dst = Ciphertext<MlKem768>;

    fn read(reader: impl Reader<'de>, dst: &mut MaybeUninit<Self::Dst>) -> ReadResult<()>
    {
        let bytes = <[u8; consts::PQ_CIPHERTEXT_SIZE] as SchemaRead<'de, C>>::get(reader)?;

        dst.write(Ciphertext::<MlKem768>::try_from(bytes.as_slice())
            .map_err(|_| ReadError::InvalidValue("not an ML-KEM-768 ciphertext"))?);

        Ok(())
    }
}

unsafe impl<C: ConfigCore> SchemaWrite<C> for BoxedOffer
{
    type Src = Box<Offer>;

    fn size_of(_: &Self::Src) -> WriteResult<usize>
    {
        Ok(2 * consts::ECC_PUBKEY_SIZE + consts::PQ_PUBKEY_SIZE + consts::ECC_SIGNATURE_SIZE)
    }

    fn write(mut writer: impl Writer, src: &Self::Src) -> WriteResult<()>
    {
        <Sec1Key as SchemaWrite<C>>::write(&mut writer, &src.static_ecc)?;
        <Sec1Key as SchemaWrite<C>>::write(&mut writer, &src.eph_ecc)?;
        <PqKey as SchemaWrite<C>>::write(&mut writer, &src.pq)?;
        <EcdsaSig as SchemaWrite<C>>::write(&mut writer, &src.sig)
    }
}

unsafe impl<'de, C: ConfigCore> SchemaRead<'de, C> for BoxedOffer
{
    type Dst = Box<Offer>;

    fn read(mut reader: impl Reader<'de>, dst: &mut MaybeUninit<Self::Dst>) -> ReadResult<()>
    {
        let static_ecc = <Sec1Key as SchemaRead<'de, C>>::get(&mut reader)?;
        let eph_ecc = <Sec1Key as SchemaRead<'de, C>>::get(&mut reader)?;
        let pq = <PqKey as SchemaRead<'de, C>>::get(&mut reader)?;
        let sig = <EcdsaSig as SchemaRead<'de, C>>::get(&mut reader)?;

        dst.write(Box::new(Offer { static_ecc, eph_ecc, pq, sig }));

        Ok(())
    }
}

unsafe impl<C: ConfigCore> SchemaWrite<C> for BoxedReply
{
    type Src = Box<Reply>;

    fn size_of(_: &Self::Src) -> WriteResult<usize>
    {
        Ok(consts::ECC_PUBKEY_SIZE + consts::PQ_CIPHERTEXT_SIZE)
    }

    fn write(mut writer: impl Writer, src: &Self::Src) -> WriteResult<()>
    {
        <Sec1Key as SchemaWrite<C>>::write(&mut writer, &src.eph_ecc)?;
        <PqCiphertext as SchemaWrite<C>>::write(&mut writer, &src.pq)
    }
}

unsafe impl<'de, C: ConfigCore> SchemaRead<'de, C> for BoxedReply
{
    type Dst = Box<Reply>;

    fn read(mut reader: impl Reader<'de>, dst: &mut MaybeUninit<Self::Dst>) -> ReadResult<()>
    {
        let eph_ecc = <Sec1Key as SchemaRead<'de, C>>::get(&mut reader)?;
        let pq = <PqCiphertext as SchemaRead<'de, C>>::get(&mut reader)?;

        dst.write(Box::new(Reply { eph_ecc, pq }));

        Ok(())
    }
}
