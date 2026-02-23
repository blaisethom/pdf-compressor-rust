use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use anyhow::{Context, Result};
use clap::Parser;
use lopdf::{Document, Object};
use pdf_compressor_rust::process_image_object;

/// Simple PDF compressor
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Input PDF file
    input: PathBuf,

    /// Output PDF file
    output: PathBuf,

    /// JPEG quality (1-100)
    #[arg(long, default_value_t = 50)]
    quality: u8,

    /// Max image dimension (longer side)
    #[arg(long, default_value_t = 1500)]
    max_dim: u32,

    /// Save debug images
    #[arg(long, default_value_t = false)]
    debug: bool,
}

fn main() -> Result<()> {
    env_logger::init();
    let args = Args::parse();

    if args.debug {
        std::fs::create_dir_all("debug_images")?;
        println!("Debug mode enabled. Images will be saved to 'debug_images/' directory.");
    }

    println!("Loading PDF: {:?}", args.input);
    let start = Instant::now();
    let mut doc = Document::load(&args.input).context("Failed to load PDF")?;
    println!("Loaded in {:.2?}", start.elapsed());

    if doc.is_encrypted() {
        println!("PDF is encrypted. Attempting to decrypt with empty password...");
        // Decrypt with empty password
        match doc.decrypt(b"") {
            Ok(_) => println!("Decrypted successfully"),
            Err(e) => {
                eprintln!("Failed to decrypt with empty password: {:?}", e);
                // If failed, continue anyway?
                // Most likely it will fail later but maybe some images are not encrypted.
            }
        }
    }

    let images_processed = AtomicUsize::new(0);
    let original_size = std::fs::metadata(&args.input)?.len();

    // Iterate over all objects to find XObject streams with Subtype = Image
    let object_ids: Vec<_> = doc.objects.keys().cloned().collect();

    // First pass: Count total images to process
    let mut total_images = 0;
    let mut processed_ids_check = std::collections::HashSet::new();

    // We need to simulate the processing logic to get an accurate count
    for object_id in &object_ids {
        if processed_ids_check.contains(object_id) {
            continue;
        }

        let (is_image, smask_id) = if let Some(Object::Stream(stream)) = doc.objects.get(object_id)
        {
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
        };

        if is_image {
            if let Some(sid) = smask_id {
                processed_ids_check.insert(sid);
            }
            processed_ids_check.insert(*object_id);
            total_images += 1;
        }
    }

    println!("Found {} images to process.", total_images);

    let mut processed_ids = std::collections::HashSet::new();
    let mut current_image_index = 0;

    for object_id in object_ids {
        if processed_ids.contains(&object_id) {
            continue;
        }

        // Check if it is an image and get smask info WITHOUT holding a borrow
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
            current_image_index += 1;

            if let Some(sid) = smask_id {
                processed_ids.insert(sid);
            }

            match process_image_object(
                &mut doc,
                object_id,
                args.quality,
                args.max_dim,
                args.debug,
                current_image_index,
            ) {
                Ok(actions) => {
                    println!(
                        "Processing image {} of {} (ID: {}): {}",
                        current_image_index, total_images, object_id.0, actions
                    );
                    images_processed.fetch_add(1, Ordering::Relaxed);
                }
                Err(e) => {
                    println!(
                        "Processing image {} of {} (ID: {}) - FAILED: {:?}",
                        current_image_index, total_images, object_id.0, e
                    );
                }
            }
            processed_ids.insert(object_id);
        }
    }

    // Save
    doc.save(&args.output).context("Failed to save PDF")?;

    let new_size = std::fs::metadata(&args.output)?.len();
    println!(
        "Processed {} images.",
        images_processed.load(Ordering::Relaxed)
    );
    println!(
        "Original size: {:.2} MB",
        original_size as f64 / 1_048_576.0
    );
    println!("New size:      {:.2} MB", new_size as f64 / 1_048_576.0);
    println!("Total processing time: {:.2?}", start.elapsed());

    Ok(())
}
