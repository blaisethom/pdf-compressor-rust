import fitz  # PyMuPDF
# Requires: pip install pymupdf Pillow
import sys
import os
import io
from PIL import Image

def compress_pdf(input_path, output_path):
    try:
        doc = fitz.open(input_path)
        processed_xrefs = set()
        
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
                        
                    # Calculate new dimensions if needed
                    # Target max dimension 1500px for screen viewing (usually good enough)
                    # If very large, downsample.
                    
                    # Handle color space
                    if pix.n - pix.alpha < 4:       # GRAY or RGB
                        pass
                    else:                           # CMYK: convert to RGB first
                        pix = fitz.Pixmap(fitz.csRGB, pix)

                    # Prepare for PIL
                    mode = "RGB"
                    if pix.alpha:
                        mode = "RGBA"
                    elif pix.n == 1:
                        mode = "L"
                        
                    img_data = pix.tobytes()
                    
                    # Convert to PIL Image
                    try:
                        pil_img = Image.frombytes(mode, [pix.width, pix.height], img_data)
                    except ValueError:
                         # Fallback for some formats
                        pil_img = Image.open(io.BytesIO(img_data))

                    # Resize if too large
                    max_dim = 1500
                    if pil_img.width > max_dim or pil_img.height > max_dim:
                        pil_img.thumbnail((max_dim, max_dim), Image.Resampling.LANCZOS)
                    
                    # Compress
                    buffer = io.BytesIO()
                    
                    if pil_img.mode == "RGBA":
                        
                        # Resize slightly more aggressively for transparent images to save space
                        max_dim_transparent = 800
                        if pil_img.width > max_dim_transparent or pil_img.height > max_dim_transparent:
                             pil_img.thumbnail((max_dim_transparent, max_dim_transparent), Image.Resampling.LANCZOS)
                        
                        # Quantize to 256 colors to reduce size significantly
                        # We need to ensure alpha is preserved. 
                        # fast_octree (method 2) or median_cut (method 0)
                        # PIL's quantize usually handles RGBA by creating a palette with alpha.
                        try:
                            pil_img = pil_img.quantize(colors=128, method=2)
                        except Exception as e:
                            print(f"Quantization failed for {xref}, using standard PNG: {e}")

                        # Preserve transparency with PNG
                        # Using optimize=True for better compression
                        pil_img.save(buffer, format="PNG", optimize=True)
                        new_image_bytes = buffer.getvalue()
                        
                        # Update the image using replace_image on the current page
                        page.replace_image(xref, stream=new_image_bytes)
                        
                        # If there was an SMask, disable it because PNG handles alpha now
                        if smask_xref > 0:
                            doc.xref_set_key(xref, "SMask", "null")
                                
                    else:
                        # JPEG for non-transparent images
                        if pil_img.mode != "RGB":
                            pil_img = pil_img.convert("RGB")
                        
                        # Resize if too large
                        max_dim = 1200
                        if pil_img.width > max_dim or pil_img.height > max_dim:
                            pil_img.thumbnail((max_dim, max_dim), Image.Resampling.LANCZOS)

                        pil_img.save(buffer, format="JPEG", quality=40, optimize=True)
                        new_image_bytes = buffer.getvalue()
                        page.replace_image(xref, stream=new_image_bytes)

                    
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
        
    except Exception as e:
        print(f"Error compressing PDF: {e}")

if __name__ == "__main__":
    if len(sys.argv) != 3:
        print("Usage: python compress_pdf.py <input_file> <output_file>")
        sys.exit(1)
        
    input_file = sys.argv[1]
    output_file = sys.argv[2]
    
    compress_pdf(input_file, output_file)
