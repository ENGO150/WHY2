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
    DynamicImage,
    AnimationDecoder,
    codecs::
    {
        gif::GifDecoder,
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

//STRUCTS
pub struct ImageFrame
{
    pub image: DynamicImage,
    pub delay: Duration, //HOW LONG IT IS HELD BEFORE THE NEXT ONE
}

//TYPES
pub type Animation = Vec<ImageFrame>; //A DECODED PICTURE

//FUNCTIONS
//PRIVATE
//DECODE LIMITS - THE WIRE SIZE BOUNDS NOTHING HERE
fn decode_limits() -> Limits
{
    let mut limits = Limits::default();

    limits.max_image_width = Some(consts::MAX_IMAGE_DIMENSION);
    limits.max_image_height = Some(consts::MAX_IMAGE_DIMENSION);
    limits.max_alloc = Some(consts::MAX_IMAGE_ALLOC);

    limits
}

fn gif_frames(data: &[u8]) -> Option<Animation>
{
    let mut decoder = GifDecoder::new(Cursor::new(data)).ok()?;

    decoder.set_limits(decode_limits()).ok()?;

    collect_frames(decoder.into_frames())
}

fn webp_frames(data: &[u8]) -> Option<Animation>
{
    let mut decoder = WebPDecoder::new(Cursor::new(data)).ok()?;

    if !decoder.has_animation() { return None; }

    decoder.set_limits(decode_limits()).ok()?;

    collect_frames(decoder.into_frames())
}

fn apng_frames(data: &[u8]) -> Option<Animation>
{
    let mut decoder = PngDecoder::new(Cursor::new(data)).ok()?;

    if !decoder.is_apng().ok()? { return None; }

    decoder.set_limits(decode_limits()).ok()?;

    collect_frames(decoder.apng().ok()?.into_frames())
}

//THE FRAMES OF ONE ANIMATION, UNDER BOTH BUDGETS
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
pub fn auto_show_images() -> bool
{
    config::read_config::<bool>("auto_show_images")
}

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

pub async fn digest_and_decode(data: Arc<Vec<u8>>) -> ([u8; 32], Option<Animation>)
{
    task::spawn_blocking(move || (crypto::sha256(&data), decode_image(&data)))
        .await.expect("Decoding image panicked")
}

pub fn fetch_image(hash: [u8; 32], tx: Sender<ClientEvent>)
{
    tokio::spawn(async move
    {
        let cached = match cache::load(&hash).await
        {
            Some(data) => task::spawn_blocking(move || decode_image(&data))
                .await.expect("Decoding image panicked"),

            None => None,
        };

        tx.send(match cached
        {
            Some(image) => ClientEvent::ImageData(hash, Some(image)),
            None => ClientEvent::ImageRequest(hash),
        }).await.unwrap();
    });
}
