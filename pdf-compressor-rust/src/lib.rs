use anyhow::{anyhow, Context, Result};
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::ColorType;
use image::{DynamicImage, GenericImageView};
use lopdf::{Document, Object, Stream};
use std::io::Write;
use wasm_bindgen::prelude::*;

fn decompress_stream(stream: &Stream, object_id: u32) -> Result<Vec<u8>> {
    match stream.decompressed_content() {
        Ok(c) => Ok(c),
        Err(e) => {
            let filter = stream.dict.get(b"Filter").ok().cloned();
            if let Some(Object::Name(name)) = filter.as_ref() {
                if name == b"FlateDecode" {
                    use std::io::Read;
                    // println!("Image {}: Attempting manual FlateDecode fallback...", object_id);
                    let mut decoder = flate2::read::ZlibDecoder::new(&stream.content[..]);
                    let mut buffer = Vec::new();
                    decoder
                        .read_to_end(&mut buffer)
                        .context("Manual zlib failed")?;
                    Ok(buffer)
                } else {
                    Err(anyhow!("Decompression failed (not Flate): {:?}", e))
                }
            } else {
                Err(anyhow!("Decompression failed (Filter type): {:?}", e))
            }
        }
    }
}

pub fn process_image_object(
    doc: &mut Document,
    object_id: (u32, u16),
    quality: u8,
    max_dim: u32,
    debug: bool,
    debug_index: u32,
) -> Result<String> {
    // Check for masks (transparency)
    let smask_id = {
        let stream = match doc.objects.get(&object_id) {
            Some(Object::Stream(s)) => s,
            _ => return Err(anyhow!("Object not a stream")),
        };

        match stream.dict.get(b"SMask") {
            Ok(Object::Reference(id)) => Some(*id),
            _ => None,
        }
    };

    let mut actions = Vec::new();

    // Resolve Filter and DecodeParms if needed
    let resolved_filter = if let Some(Object::Stream(stream)) = doc.objects.get(&object_id) {
        match stream.dict.get(b"Filter") {
            Ok(Object::Reference(id)) => Some(doc.objects.get(id).cloned().unwrap_or(Object::Null)),
            Ok(Object::Array(arr)) => {
                let mut new_arr = Vec::new();
                let mut changed = false;
                for item in arr {
                    if let Object::Reference(id) = item {
                        if let Some(obj) = doc.objects.get(id) {
                            new_arr.push(obj.clone());
                            changed = true;
                        } else {
                            new_arr.push(item.clone());
                        }
                    } else {
                        new_arr.push(item.clone());
                    }
                }
                if changed {
                    Some(Object::Array(new_arr))
                } else {
                    None
                }
            }
            _ => None,
        }
    } else {
        None
    };

    let resolved_parms = if let Some(Object::Stream(stream)) = doc.objects.get(&object_id) {
        match stream.dict.get(b"DecodeParms") {
            Ok(Object::Reference(id)) => Some(doc.objects.get(id).cloned().unwrap_or(Object::Null)),
            Ok(Object::Array(arr)) => {
                let mut new_arr = Vec::new();
                let mut changed = false;
                for item in arr {
                    if let Object::Reference(id) = item {
                        if let Some(obj) = doc.objects.get(id) {
                            new_arr.push(obj.clone());
                            changed = true;
                        } else {
                            new_arr.push(item.clone());
                        }
                    } else {
                        new_arr.push(item.clone());
                    }
                }
                if changed {
                    Some(Object::Array(new_arr))
                } else {
                    None
                }
            }
            _ => None,
        }
    } else {
        None
    };

    // Update the stream with resolved values
    if let Some(val) = resolved_filter {
        if let Some(Object::Stream(stream)) = doc.objects.get_mut(&object_id) {
            stream.dict.set("Filter", val);
        }
    }
    if let Some(val) = resolved_parms {
        if let Some(Object::Stream(stream)) = doc.objects.get_mut(&object_id) {
            stream.dict.set("DecodeParms", val);
        }
    }

    // Extract the stream and decode it
    let (width, height, components, content, color_space_name) = {
        let stream = match doc.objects.get_mut(&object_id) {
            Some(Object::Stream(s)) => s,
            _ => return Err(anyhow!("Object not a stream")),
        };
        // println!("Processing Image {}: Filter={:?}", object_id.0, stream.dict.get(b"Filter"));

        let filter = stream.dict.get(b"Filter").ok().cloned();

        // Helper to check if filter list contains DCTDecode
        let is_jpeg = match &filter {
            Some(Object::Name(name)) => name == b"DCTDecode",
            Some(Object::Array(arr)) => arr.iter().any(|o| match o {
                Object::Name(name) => name == b"DCTDecode",
                _ => false,
            }),
            _ => false,
        };

        if is_jpeg {
            actions.push("was JPEG".to_string());
        }

        let content = if is_jpeg {
            match stream.decompressed_content() {
                Ok(c) => c,
                Err(_) => {
                    // Fallback: if it's DCTDecode, return raw content
                    stream.content.clone()
                }
            }
        } else {
            decompress_stream(stream, object_id.0)?
        };

        let dict = &stream.dict;
        let w = dict.get(b"Width").and_then(|o| o.as_i64()).unwrap_or(0) as u32;
        let h = dict.get(b"Height").and_then(|o| o.as_i64()).unwrap_or(0) as u32;
        let cs = dict.get(b"ColorSpace").ok().and_then(|o| match o {
            Object::Name(n) => Some(n.clone()),
            _ => None,
        });

        // Simple component count heuristic
        let len = content.len();
        let c = if let Some(ref name) = cs {
            if name == b"DeviceGray" {
                1
            } else if name == b"DeviceRGB" {
                3
            } else if name == b"DeviceCMYK" {
                4
            } else {
                3
            } // Assume RGB if complex?
        } else {
            if len == (w * h) as usize {
                1
            } else if len == (w * h * 3) as usize {
                3
            } else if len == (w * h * 4) as usize {
                4
            } else {
                3
            } // Default
        };

        (w, h, c, content, cs)
    };

    // Decode image to DynamicImage
    let mut img = if components == 0 {
        image::load_from_memory(&content).context("Failed to load image from memory")?
    } else {
        match components {
            1 => DynamicImage::ImageLuma8(
                image::GrayImage::from_raw(width, height, content.clone())
                    .or_else(|| image::load_from_memory(&content).map(|i| i.to_luma8()).ok())
                    .ok_or(anyhow!("Failed Gray"))?,
            ),
            3 => DynamicImage::ImageRgb8(
                image::RgbImage::from_raw(width, height, content.clone())
                    .or_else(|| image::load_from_memory(&content).map(|i| i.to_rgb8()).ok())
                    .ok_or(anyhow!("Failed RGB"))?,
            ),
            4 => {
                actions.push("CMYK->RGB".to_string());
                if let Ok(img) = image::load_from_memory(&content) {
                    img
                } else {
                    let rgb: Vec<u8> = content
                        .chunks(4)
                        .flat_map(|cmyk| {
                            if cmyk.len() < 4 {
                                return vec![0, 0, 0];
                            }
                            let c = cmyk[0] as f32 / 255.0;
                            let m = cmyk[1] as f32 / 255.0;
                            let y = cmyk[2] as f32 / 255.0;
                            let k = cmyk[3] as f32 / 255.0;

                            let r = (1.0 - c) * (1.0 - k);
                            let g = (1.0 - m) * (1.0 - k);
                            let b = (1.0 - y) * (1.0 - k);

                            vec![(r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8]
                        })
                        .collect();
                    DynamicImage::ImageRgb8(
                        image::RgbImage::from_raw(width, height, rgb)
                            .ok_or(anyhow!("Failed CMYK->RGB"))?,
                    )
                }
            }
            _ => return Err(anyhow!("Unsupported components {}", components)),
        }
    };

    if debug {
        let path = format!("debug_images/Image{}-before.png", debug_index);
        if let Err(e) = img.save(&path) {
            eprintln!("Failed to save debug image {}: {:?}", path, e);
        }
    }

    // Handle SMask (Transparency)
    if let Some(smask_id) = smask_id {
        actions.push("applied SMask".to_string());
        let (mw, mh, mcontent) = {
            let stream = match doc.objects.get(&smask_id) {
                Some(Object::Stream(s)) => s,
                _ => return Err(anyhow!("SMask not a stream")),
            };
            let content =
                decompress_stream(stream, smask_id.0).context("Failed to decompress mask")?;
            let dict = &stream.dict;
            let w = dict.get(b"Width").and_then(|o| o.as_i64()).unwrap_or(0) as u32;
            let h = dict.get(b"Height").and_then(|o| o.as_i64()).unwrap_or(0) as u32;
            (w, h, content)
        };

        if mw == width && mh == height {
            let mask =
                image::GrayImage::from_raw(mw, mh, mcontent).ok_or(anyhow!("Failed Mask"))?;

            if debug {
                let path = format!("debug_images/Image{}-mask-extracted.png", debug_index);
                if let Err(e) = mask.save(&path) {
                    eprintln!("Failed to save mask image {}: {:?}", path, e);
                }
            }

            // Convert main image to RGBA
            let mut rgba = img.to_rgba8();

            // Apply mask
            for (x, y, pixel) in rgba.enumerate_pixels_mut() {
                let mask_pixel = mask.get_pixel(x, y);
                pixel[3] = mask_pixel[0];
            }

            img = DynamicImage::ImageRgba8(rgba);
        }
    }

    // Resize
    let img = if img.width() > max_dim || img.height() > max_dim {
        actions.push(format!("resize {}x{} -> ", img.width(), img.height()));
        let new_img = img.resize(max_dim, max_dim, FilterType::Lanczos3);
        actions
            .last_mut()
            .unwrap()
            .push_str(&format!("{}x{}", new_img.width(), new_img.height()));
        new_img
    } else {
        actions.push(format!("keep dims {}x{}", img.width(), img.height()));
        img
    };

    let (w, h) = img.dimensions();

    if debug {
        let path = format!("debug_images/Image{}-after.jpg", debug_index);
        if let Err(e) = img.save(&path) {
            eprintln!("Failed to save debug image {}: {:?}", path, e);
        }
    }

    // Re-encode
    if let Some(smask_id) = smask_id {
        actions.push("re-encode: Split RGB(JPEG) + Alpha(Flate)".to_string());
        // Has transparency. Split into RGB (JPEG) and Alpha (Flate)
        let rgba = img.to_rgba8();

        // Extract RGB and Alpha channels
        let mut rgb_pixels = Vec::with_capacity((w * h * 3) as usize);
        let mut alpha_pixels = Vec::with_capacity((w * h) as usize);

        for pixel in rgba.pixels() {
            rgb_pixels.push(pixel[0]);
            rgb_pixels.push(pixel[1]);
            rgb_pixels.push(pixel[2]);
            alpha_pixels.push(pixel[3]);
        }

        // 1. Update Main Image (RGB + JPEG)
        let mut buffer = Vec::new();
        let mut encoder = JpegEncoder::new_with_quality(&mut buffer, quality);
        encoder.encode(&rgb_pixels, w, h, ColorType::Rgb8.into())?;

        if let Some(Object::Stream(stream)) = doc.objects.get_mut(&object_id) {
            stream
                .dict
                .set("Length", Object::Integer(buffer.len() as i64));
            stream.content = buffer;
            stream
                .dict
                .set("Filter", Object::Name(b"DCTDecode".to_vec()));
            stream.dict.set("Width", Object::Integer(w as i64));
            stream.dict.set("Height", Object::Integer(h as i64));
            stream
                .dict
                .set("ColorSpace", Object::Name(b"DeviceRGB".to_vec()));
            stream.dict.set("BitsPerComponent", Object::Integer(8));
            stream.dict.remove(b"DecodeParms"); // Remove old params
            stream.dict.remove(b"Decode"); // Remove potential Decode array
                                           // stream.dict.remove(b"Length"); // Remove length so it is recalculated
        }

        // 2. Update Mask (Alpha + Flate)
        // Flate compression for mask
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
        encoder.write_all(&alpha_pixels)?;
        let compressed_mask = encoder.finish()?;

        if let Some(Object::Stream(stream)) = doc.objects.get_mut(&smask_id) {
            let mask_len = compressed_mask.len();
            stream.content = compressed_mask;
            stream
                .dict
                .set("Filter", Object::Name(b"FlateDecode".to_vec()));
            stream.dict.set("Width", Object::Integer(w as i64));
            stream.dict.set("Height", Object::Integer(h as i64));
            stream
                .dict
                .set("ColorSpace", Object::Name(b"DeviceGray".to_vec()));
            stream.dict.set("BitsPerComponent", Object::Integer(8));
            stream.dict.remove(b"DecodeParms");
            // Ensure no Decode array is messing things up, or force default [0, 1]
            stream.dict.remove(b"Decode");
            // stream.dict.remove(b"Length"); // Remove length so it is recalculated
            stream.dict.set("Length", Object::Integer(mask_len as i64));
        }
    } else {
        // No transparency (Opaque)
        actions.push(format!("re-encode: JPEG(q={})", quality));
        let mut buffer = Vec::new();
        let mut encoder = JpegEncoder::new_with_quality(&mut buffer, quality);
        encoder.encode_image(&img)?;

        // Update the stream
        if let Some(Object::Stream(stream)) = doc.objects.get_mut(&object_id) {
            stream
                .dict
                .set("Length", Object::Integer(buffer.len() as i64));
            stream.content = buffer;
            stream
                .dict
                .set("Filter", Object::Name(b"DCTDecode".to_vec()));
            stream.dict.set("Width", Object::Integer(w as i64));
            stream.dict.set("Height", Object::Integer(h as i64));
            stream
                .dict
                .set("ColorSpace", Object::Name(b"DeviceRGB".to_vec()));
            stream.dict.set("BitsPerComponent", Object::Integer(8));
            stream.dict.remove(b"DecodeParms");
            // stream.dict.remove(b"Length"); // Remove length so it is recalculated
            println!(
                "DEBUG: Image {} (opaque) has Length: {:?}",
                object_id.0,
                stream.dict.get(b"Length")
            );
        }
    }

    Ok(actions.join(", "))
}

#[wasm_bindgen]
pub fn compress_pdf(input: &[u8], quality: u8, max_dim: u32) -> Result<Vec<u8>, JsError> {
    // Initialize console_error_panic_hook for better error messages in browser console
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();

    let mut doc = Document::load_from(std::io::Cursor::new(input))
        .map_err(|e| JsError::new(&format!("Failed to load PDF: {:?}", e)))?;

    if doc.is_encrypted() {
        match doc.decrypt(b"") {
            Ok(_) => {} // web_sys::console::log_1(&"Decrypted successfully".into()),
            Err(_) => {
                // web_sys::console::warn_1(&format!("Failed to decrypt: {:?}", e).into());
            }
        }
    }

    let object_ids: Vec<_> = doc.objects.keys().cloned().collect();
    let mut processed_ids = std::collections::HashSet::new();

    for object_id in object_ids {
        if processed_ids.contains(&object_id) {
            continue;
        }

        let (is_image, smask_id) = {
            if let Some(Object::Stream(stream)) = doc.objects.get(&object_id) {
                if let Ok(subtype) = stream.dict.get(b"Subtype") {
                    if let Ok(name) = subtype.as_name() {
                        if name == b"Image" {
                            let smask = match stream.dict.get(b"SMask") {
                                Ok(Object::Reference(id)) => Some(*id),
                                _ => None,
                            };
                            (true, smask)
                        } else {
                            (false, None)
                        }
                    } else {
                        (false, None)
                    }
                } else {
                    (false, None)
                }
            } else {
                (false, None)
            }
        };

        if is_image {
            if let Some(sid) = smask_id {
                processed_ids.insert(sid);
            }

            if let Err(e) = process_image_object(&mut doc, object_id, quality, max_dim, false, 0) {
                // web_sys::console::error_1(&format!("Failed to process image {}: {:?}", object_id.0, e).into());
            }
            processed_ids.insert(object_id);
        }
    }

    let mut buffer = Vec::new();
    doc.save_to(&mut buffer)
        .map_err(|e| JsError::new(&format!("Failed to save PDF: {:?}", e)))?;

    Ok(buffer)
}
