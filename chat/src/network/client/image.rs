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

//! # Picture Decoding
//!
//! Bounded decoding of chat pictures, and the client-side cutting of avatars.
//!
//! # Bounds
//! A picture's size on the wire bounds nothing about what it decodes to — a 292 KB PNG can
//! unpack to 400 MB — and pictures are pushed to every client in a channel unasked. Every
//! decode is therefore limited to [`MAX_IMAGE_DIMENSION`](consts::MAX_IMAGE_DIMENSION)
//! per side and [`MAX_IMAGE_ALLOC`](consts::MAX_IMAGE_ALLOC) per frame. An animation is bounded
//! again as a whole, by [`MAX_ANIMATION_FRAMES`](consts::MAX_ANIMATION_FRAMES) and
//! [`MAX_ANIMATION_ALLOC`](consts::MAX_ANIMATION_ALLOC); running out of budget truncates the
//! animation rather than failing it.
//!
//! # Frames
//! A decoded picture is always an [`Animation`]: a still is one frame. Animated GIF, WebP and
//! APNG are decoded frame by frame; everything else, and an animated format with a single
//! frame, decodes as one.

use std::
{
    sync::Arc,
    io::Cursor,
    time::Duration,
};

use tokio::
{
    task,
    sync::mpsc::Sender,
};

use image::
{
    Frames,
    Limits,
    ImageFormat,
    ImageReader,
    ImageDecoder,
    Delay,
    Frame,
    DynamicImage,
    AnimationDecoder,
    imageops::FilterType,
    codecs::
    {
        gif::{ GifDecoder, GifEncoder, Repeat },
        png::PngDecoder,
        webp::WebPDecoder,
    },
};

use crate::
{
    cache,
    crypto,
    config,
    consts,
    network::client::ClientEvent,
};

#[cfg(feature = "client_voice")]
use super::media;

//STRUCTS
/// One frame of a decoded picture.
#[derive(Clone)]
pub struct ImageFrame
{
    /// The frame's pixels.
    pub image: DynamicImage,
    /// How long the frame is shown before the next one; zero for a still.
    pub delay: Duration, //HOW LONG IT IS HELD BEFORE THE NEXT ONE
}

//TYPES
/// A decoded picture: one frame for a still, several for an animation.
pub type Animation = Vec<ImageFrame>; //A DECODED PICTURE

//FUNCTIONS
//PRIVATE
//DECODE LIMITS - THE WIRE SIZE BOUNDS NOTHING HERE
/// Returns the per-picture decode limits.
fn decode_limits() -> Limits
{
    let mut limits = Limits::default();

    limits.max_image_width = Some(consts::MAX_IMAGE_DIMENSION);
    limits.max_image_height = Some(consts::MAX_IMAGE_DIMENSION);
    limits.max_alloc = Some(consts::MAX_IMAGE_ALLOC);

    limits
}

/// Decodes the frames of a GIF.
fn gif_frames(data: &[u8]) -> Option<Animation>
{
    let mut decoder = GifDecoder::new(Cursor::new(data)).ok()?;

    decoder.set_limits(decode_limits()).ok()?;

    collect_frames(decoder.into_frames())
}

/// Decodes the frames of an animated WebP; `None` for a still one.
fn webp_frames(data: &[u8]) -> Option<Animation>
{
    let mut decoder = WebPDecoder::new(Cursor::new(data)).ok()?;

    if !decoder.has_animation() { return None; }

    decoder.set_limits(decode_limits()).ok()?;

    collect_frames(decoder.into_frames())
}

/// Decodes the frames of an APNG; `None` for a plain PNG.
fn apng_frames(data: &[u8]) -> Option<Animation>
{
    let mut decoder = PngDecoder::new(Cursor::new(data)).ok()?;

    if !decoder.is_apng().ok()? { return None; }

    decoder.set_limits(decode_limits()).ok()?;

    collect_frames(decoder.apng().ok()?.into_frames())
}

//THE FRAMES OF ONE ANIMATION, UNDER BOTH BUDGETS
/// Collects the frames of an animation under the frame-count and memory budgets.
///
/// A frame that fails to decode, or one past a budget, ends the animation with what was already
/// decoded. A delay shorter than [`MIN_FRAME_DELAY`](consts::MIN_FRAME_DELAY) is replaced with
/// [`DEFAULT_FRAME_DELAY`](consts::DEFAULT_FRAME_DELAY).
///
/// # Returns
/// - `Some(frames)` with at least one frame.
/// - `None` if not a single frame decoded.
fn collect_frames(frames: Frames<'_>) -> Option<Animation>
{
    let mut animation: Animation = Vec::new();
    let mut alloc = 0u64;

    for frame in frames.take(consts::MAX_ANIMATION_FRAMES)
    {
        let Ok(frame) = frame else { break };

        let delay = match frame.delay().numer_denom_ms()
        {
            (_, 0) => consts::DEFAULT_FRAME_DELAY,
            (numer, denom) => Duration::from_micros(numer as u64 * 1_000 / denom as u64),
        };

        let image = DynamicImage::from(frame.into_buffer());

        alloc += image.width() as u64 * image.height() as u64 * 4;

        if alloc > consts::MAX_ANIMATION_ALLOC && !animation.is_empty() { break; }

        animation.push(ImageFrame
        {
            image,
            delay: match delay < consts::MIN_FRAME_DELAY
            {
                true => consts::DEFAULT_FRAME_DELAY,
                false => delay,
            },
        });
    }

    match animation.is_empty()
    {
        true => None,
        false => Some(animation),
    }
}

//PUBLIC
//WHETHER A PICTURE IS DRAWN AS IT ARRIVES
/// Returns whether pictures are decoded and shown as they arrive (`auto_show_images`).
pub fn auto_show_images() -> bool
{
    config::read_config::<bool>("auto_show_images")
}

/// Decodes a picture within the decode limits.
///
/// This is CPU-heavy; async callers run it in `spawn_blocking`.
///
/// # Parameters
/// - `data`: The encoded picture.
///
/// # Returns
/// - `Some(frames)` — every frame of an animation, or the single frame of a still.
/// - `None` if the format is unknown, the picture is over a limit, or it does not decode.
pub fn decode_image(data: &[u8]) -> Option<Animation>
{
    let mut reader = ImageReader::new(Cursor::new(data)).with_guessed_format().ok()?;

    reader.limits(decode_limits());

    //ONLY AN ANIMATED FORMAT IS DECODED AS FRAMES
    let animated = match reader.format()
    {
        Some(ImageFormat::Gif) => gif_frames(data),
        Some(ImageFormat::WebP) => webp_frames(data),
        Some(ImageFormat::Png) => apng_frames(data),

        _ => None,
    };

    if let Some(frames) = animated && frames.len() > 1 { return Some(frames); }

    Some(vec![ImageFrame { image: reader.decode().ok()?, delay: Duration::ZERO }])
}

//CUT A PICTURE TO THE CENTRED SQUARE AN AVATAR IS, AS A PNG OR AN ANIMATED GIF
/// Cuts a picture into an avatar: the centred square, scaled down to at most
/// [`AVATAR_DIMENSION`](consts::AVATAR_DIMENSION) (or
/// [`ANIMATED_AVATAR_DIMENSION`](consts::ANIMATED_AVATAR_DIMENSION) for an animation).
///
/// The result is always a PNG or an animated GIF — the only formats the server's header check
/// ([`is_avatar`](crate::misc::is_avatar)) accepts.
///
/// # Returns
/// - `Some((bytes, extension))` with the encoded avatar and `"png"` or `"gif"`.
/// - `None` if the picture does not decode or encode.
pub fn make_avatar(data: &[u8]) -> Option<(Vec<u8>, &'static str)>
{
    let frames = decode_image(data)?;
    let first = &frames.first()?.image;

    let side = first.width().min(first.height());

    if side == 0 { return None; }

    let (x, y) = ((first.width() - side) / 2, (first.height() - side) / 2);
    let animated = frames.len() > 1;

    let target = side.min(if animated { consts::ANIMATED_AVATAR_DIMENSION } else { consts::AVATAR_DIMENSION });
    let square = |image: &DynamicImage| image.crop_imm(x, y, side, side).resize_exact(target, target, FilterType::Triangle);

    let mut out = Vec::new();

    match animated
    {
        false =>
        {
            square(first).write_to(&mut Cursor::new(&mut out), ImageFormat::Png).ok()?;

            Some((out, "png"))
        },

        true =>
        {
            {
                let mut encoder = GifEncoder::new_with_speed(&mut out, consts::AVATAR_GIF_SPEED);
                encoder.set_repeat(Repeat::Infinite).ok()?;

                encoder.encode_frames(frames.iter().map(|frame| Frame::from_parts(square(&frame.image).to_rgba8(),
                    0, 0, Delay::from_saturating_duration(frame.delay)))).ok()?;
            }

            Some((out, "gif"))
        },
    }
}

/// Hashes and decodes an attachment preview on a blocking thread.
///
/// # Returns
/// The attachment's SHA-256 and its image frames or video poster, if supported.
///
/// # Panics
/// If the blocking task panics.
pub async fn digest_and_decode(data: Arc<Vec<u8>>) -> ([u8; 32], Option<Animation>)
{
    task::spawn_blocking(move || (crypto::sha256(&data), decode_attachment(&data)))
        .await.expect("Decoding image panicked")
}

//DECODE AN IMAGE OR A VIDEO POSTER WITHOUT STARTING ITS PLAYER
/// Decodes an attachment preview; video posters are available with `client_voice`.
pub fn decode_attachment(data: &[u8]) -> Option<Animation>
{
    if let Some(image) = decode_image(data) { return Some(image); }

    #[cfg(feature = "client_voice")]
    { media::thumbnail(data) }

    #[cfg(not(feature = "client_voice"))]
    { None }
}

/// Loads a picture from the cache, or asks for it to be fetched from the server.
///
/// Spawns a task that answers with [`ClientEvent::ImageData`] on a cache hit, or
/// [`ClientEvent::AttachmentRequest`] on a miss, which the UI turns into a server request.
///
/// # Parameters
/// - `hash`: The picture's SHA-256.
/// - `tx`: The UI event channel.
pub fn fetch_image(hash: [u8; 32], tx: Sender<ClientEvent>)
{
    tokio::spawn(async move
    {
        let event = match cache::load(&hash).await
        {
            Some(data) =>
            {
                let image = task::spawn_blocking(move || decode_attachment(&data))
                    .await.expect("Decoding attachment preview panicked");

                ClientEvent::ImageData { hash, image }
            },

            None => ClientEvent::AttachmentRequest { hash },
        };

        tx.send(event).await.unwrap();
    });
}
