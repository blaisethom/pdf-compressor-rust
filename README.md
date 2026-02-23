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

This project is also designed to be compiled to WebAssembly for use in the browser.
