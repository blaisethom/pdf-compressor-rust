import fitz  # PyMuPDF
# Requires: pip install pymupdf Pillow
import sys
import os
import io
import time
from PIL import Image

import argparse

def compress_pdf(input_path, output_path, debug=False):
    start_time = time.time()
    try:
        if debug:
            os.makedirs("debug_images", exist_ok=True)
            print("Debug mode enabled. Images will be saved to 'debug_images/' directory.")

        doc = fitz.open(input_path)
        
        # Count total unique images first
        unique_xrefs = set()
        for page_num in range(len(doc)):
            for img in doc[page_num].get_images():
                unique_xrefs.add(img[0])
        total_images = len(unique_xrefs)
        print(f"Found {total_images} unique images to process.")
        
        processed_xrefs = set()
        processed_count = 0
        
        # Calculate target size per page or overall strategy
        # For 64MB -> 10MB, we need aggressive compression (factor of 6).
        # We'll downsample large images and use JPEG compression.
        
        for page_num in range(len(doc)):
            page = doc[page_num]
            image_list = page.get_images()
            
            for img in image_list:
                xref = img[0]
                smask_xref = img[1]
                
                if xref in processed_xrefs:
                    continue
                
                # Check if we should process this image
                processed_xrefs.add(xref)
                processed_count += 1
                
                try:
                    # Create a pixmap from the image reference
                    # If there's an smask, we need to handle it to preserve transparency
                    if smask_xref > 0:
                        # Construct pixmap with mask
                        pix0 = fitz.Pixmap(doc, xref)
                        mask = fitz.Pixmap(doc, smask_xref)
                        try:
                            pix = fitz.Pixmap(pix0, mask)
                        except:
                            # Fallback if mask composition fails
                            pix = fitz.Pixmap(doc, xref)
                    else:
                        pix = fitz.Pixmap(doc, xref)
                    
                    # Skip tiny images
                    if pix.width < 100 or pix.height < 100:
                        continue
                    
                    original_dims = f"{pix.width}x{pix.height}"
                    actions = []

                    # Calculate new dimensions if needed
                    # Target max dimension 1500px for screen viewing (usually good enough)
                    # If very large, downsample.
                    
                    # Handle color space
                    if pix.n - pix.alpha < 4:       # GRAY or RGB
                        pass
                    else:                           # CMYK: convert to RGB first
                        pix = fitz.Pixmap(fitz.csRGB, pix)
                        actions.append("CMYK->RGB")

                    # Prepare for PIL
                    mode = "RGB"
                    if pix.alpha:
                        mode = "RGBA"
                    elif pix.n == 1:
                        mode = "L"
                        
                    img_data = pix.tobytes()
                    
                    # Convert to PIL Image
                    try:
                        pil_img = Image.frombytes(mode, (pix.width, pix.height), img_data)
                    except ValueError:
                         # Fallback for some formats
                        pil_img = Image.open(io.BytesIO(img_data))
                    
                    if debug:
                        debug_before_path = os.path.join("debug_images", f"Image{processed_count}-before.png")
                        pil_img.save(debug_before_path)
                        actions.append(f"saved {debug_before_path}")

                    # Resize if too large
                    max_dim = 1500
                    if pil_img.width > max_dim or pil_img.height > max_dim:
                        pil_img.thumbnail((max_dim, max_dim), Image.Resampling.LANCZOS)
                        actions.append(f"resize {original_dims}->{pil_img.width}x{pil_img.height}")
                    else:
                        actions.append(f"keep dims {original_dims}")
                    
                    # Compress
                    buffer = io.BytesIO()
                    
                    if pil_img.mode == "RGBA":
                        
                        # Resize slightly more aggressively for transparent images to save space
                        max_dim_transparent = 800
                        if pil_img.width > max_dim_transparent or pil_img.height > max_dim_transparent:
                             pil_img.thumbnail((max_dim_transparent, max_dim_transparent), Image.Resampling.LANCZOS)
                             actions.append(f"resize (transparent) ->{pil_img.width}x{pil_img.height}")
                        
                        # Quantize to 256 colors to reduce size significantly
                        # We need to ensure alpha is preserved. 
                        # fast_octree (method 2) or median_cut (method 0)
                        # PIL's quantize usually handles RGBA by creating a palette with alpha.
                        try:
                            pil_img = pil_img.quantize(colors=128, method=2)
                            actions.append("quantize (128 colors)")
                        except Exception as e:
                            print(f"Quantization failed for {xref}, using standard PNG: {e}")

                        # Preserve transparency with PNG
                        # Using optimize=True for better compression
                        pil_img.save(buffer, format="PNG", optimize=True)
                        actions.append("format: PNG")
                        new_image_bytes = buffer.getvalue()
                        
                        if debug:
                            debug_after_path = os.path.join("debug_images", f"Image{processed_count}-after.png")
                            pil_img.save(debug_after_path)
                            actions.append(f"saved {debug_after_path}")
                        
                        # Update the image using replace_image on the current page
                        page.replace_image(xref, stream=new_image_bytes)
                        
                        # If there was an SMask, disable it because PNG handles alpha now
                        if smask_xref > 0:
                            doc.xref_set_key(xref, "SMask", "null")
                            actions.append("removed SMask")
                                
                    else:
                        # JPEG for non-transparent images
                        if pil_img.mode != "RGB":
                            pil_img = pil_img.convert("RGB")
                            actions.append(f"convert {mode}->RGB")
                        
                        # Resize if too large
                        max_dim = 1200
                        if pil_img.width > max_dim or pil_img.height > max_dim:
                            pil_img.thumbnail((max_dim, max_dim), Image.Resampling.LANCZOS)
                            actions.append(f"resize ->{pil_img.width}x{pil_img.height}")

                        pil_img.save(buffer, format="JPEG", quality=40, optimize=True)
                        actions.append("format: JPEG (q=40)")
                        new_image_bytes = buffer.getvalue()
                        
                        if debug:
                            debug_after_path = os.path.join("debug_images", f"Image{processed_count}-after.jpg")
                            pil_img.save(debug_after_path)
                            actions.append(f"saved {debug_after_path}")

                        page.replace_image(xref, stream=new_image_bytes)
                    
                    print(f"Processing image {processed_count} of {total_images} (xref {xref}): {', '.join(actions)}")


                    
                    # Update the image using replace_image on the current page
                    # This updates the global XObject so it affects all pages using it.
                    page.replace_image(xref, stream=new_image_bytes)
                    
                except Exception as e:
                    print(f"Warning: Could not process image xref {xref}: {e}")
                    continue

        # Save with garbage collection and deflate
        doc.save(output_path, garbage=4, deflate=True)
        
        original_size = os.path.getsize(input_path)
        new_size = os.path.getsize(output_path)
        
        print(f"Original size: {original_size / (1024*1024):.2f} MB")
        print(f"Compressed size: {new_size / (1024*1024):.2f} MB")
        
        end_time = time.time()
        print(f"Total processing time: {end_time - start_time:.2f} seconds")
        
    except Exception as e:
        print(f"Error compressing PDF: {e}")

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Compress a PDF file.")
    parser.add_argument("input", help="Input PDF file")
    parser.add_argument("output", help="Output PDF file")
    parser.add_argument("--debug", action="store_true", help="Save debug images")
    
    args = parser.parse_args()
    
    compress_pdf(args.input, args.output, args.debug)
