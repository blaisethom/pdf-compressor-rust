# PDF Compressor (Rust)

A high-performance PDF compression tool written in Rust. It reduces file size by downsampling and re-encoding images, with support for transparent images (SMask).

## Features

- **High Compression:** Significantly reduces PDF size by re-encoding images to JPEG.
- **Transparency Support:** Preserves transparency in images (which standard PDF compressors often break) by separating RGB and Alpha channels.
- **Configurable:** Adjustable JPEG quality and maximum image dimensions.
- **Fast:** Parallel processing capable (future enhancement) and efficient memory usage.

## Prerequisites

- **Rust:** You need to have Rust installed. If not, install it from [rustup.rs](https://rustup.rs/).

## Building

To build the project for the command line:

```bash
cd pdf-compressor-rust
cargo build --release
```

The compiled binary will be located at `target/release/pdf-compressor-rust`.

## Usage

Run the compressor using the built binary:

```bash
./target/release/pdf-compressor-rust <INPUT_PDF> <OUTPUT_PDF> [OPTIONS]
```

### Arguments

- `<INPUT_PDF>`: Path to the source PDF file.
- `<OUTPUT_PDF>`: Path where the compressed PDF will be saved.

### Options

- `--quality <u8>`: JPEG quality (1-100). Default is `50`. Lower values mean smaller size but lower quality.
- `--max-dim <u32>`: Maximum dimension (width or height) for images. Default is `1500` pixels. Images larger than this will be resized.

### Example

Compress "input.pdf" to "output.pdf" with quality 60 and max dimension 2000:

```bash
./target/release/pdf-compressor-rust "input.pdf" "output.pdf" --quality 60 --max-dim 2000
```

## Troubleshooting

If you see "Processed 0 images" or get no compression:
1. Ensure the PDF actually contains raster images (not just vector graphics).
2. Ensure you are running the latest build (`cargo build --release`).
3. Some PDFs use complex object structures that might not be detected yet.

## WebAssembly (WASM)

This project can be compiled to WebAssembly for client-side use in the browser.

### Building for Web

You need `wasm-pack` installed:
```bash
curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh
```

Then build:
```bash
cd pdf-compressor-rust
# For direct browser use (ES modules):
wasm-pack build --target web
# For bundlers (Webpack, Vite, Rollup):
wasm-pack build --target bundler
```

### Usage (Direct Browser)

If you built with `--target web`:

```html
<script type="module">
  import init, { compress_pdf } from './pkg/pdf_compressor_rust.js';

  async function run() {
    await init();
    
    // Load your PDF as Uint8Array (e.g., from file input)
    const fileInput = document.getElementById('file-input');
    const file = fileInput.files[0];
    const arrayBuffer = await file.arrayBuffer();
    const pdfBytes = new Uint8Array(arrayBuffer);

    try {
      // Compress: (input_bytes, quality_1_100, max_dimension)
      const compressedBytes = compress_pdf(pdfBytes, 50, 1500);
      
      // Download or display compressedBytes
      const blob = new Blob([compressedBytes], { type: 'application/pdf' });
      const url = URL.createObjectURL(blob);
      window.open(url);
    } catch (e) {
      console.error("Compression failed:", e);
    }
  }
</script>
```

### Usage (Bundlers / React / Vue)

If you built with `--target bundler` and installed the package:

```javascript
import * as wasm from "pdf-compressor-rust";

// Note: In some setups you might need to handle WASM loading asynchronously
// or use a plugin like vite-plugin-wasm.

const compress = (pdfBytes) => {
    // quality=50, max_dim=1500
    return wasm.compress_pdf(pdfBytes, 50, 1500);
};
```

### CI/CD

A GitHub Action is set up in `.github/workflows/wasm-release.yml` to automatically build and release the WASM package on every tag push (e.g., `v0.1.0`). It produces artifacts for both `web` and `bundler` targets.

