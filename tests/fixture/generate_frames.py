#!/usr/bin/env python3
"""Generate deterministic synthetic test frames as 24-bit BMP files.

Purpose (see docs/DESIGN.md §16.1):
    The encode-pipeline acceptance tests feed synthetic D3D textures -- solid
    color, checkerboard, and gradient -- into the H.264/MP4 pipeline, then use
    ffprobe (dev/CI only) to verify resolution, frame rate, duration and audio
    track.  Synthetic frames are reproducible, privacy-free and easy to assert
    against.  This script produces the BMP pixel data for those patterns.

Only the Python standard library is used (struct, argparse, os).  The output is
an uncompressed 24-bit BMP (bottom-up BGR rows, 4-byte row padding), so test
code can read raw pixels with no external decoder.

Usage:
    python generate_frames.py --width 1920 --height 1080 --count 12 --out frames
    python generate_frames.py --width 1280 --height 720 --count 4 --out frames --pattern checkerboard

Arguments:
    --width    image width in pixels (default 1920)
    --height   image height in pixels (default 1080)
    --count    number of frames to generate (default 3)
    --out      output directory (default "frames")
    --pattern  solid | checkerboard | gradient | all (default "all"; cycles
               through the three patterns when set to all)
"""

import argparse
import os
import struct

FILE_HEADER_SIZE = 14
INFO_HEADER_SIZE = 40
PIXEL_OFFSET = FILE_HEADER_SIZE + INFO_HEADER_SIZE  # 54


def _solid(width, height, frame):
    """Uniform color that shifts per frame so frame ordering is detectable."""
    base = (frame * 37) % 256
    r, g, b = base, 255 - base, (base // 2) + 40
    return lambda x, y: (r, g, b)


def _checkerboard(width, height, frame):
    """32px checkerboard; tone shifts slightly each frame."""
    block = 32
    tone = (frame * 23) % 96
    c_a = (tone, tone, tone)
    c_b = (200, 120 + tone // 2, 40)
    return lambda x, y: c_a if ((x // block) + (y // block)) % 2 == 0 else c_b


def _gradient(width, height, frame):
    """Horizontal red / vertical green gradient with a frame-varying blue."""
    w = max(width - 1, 1)
    h = max(height - 1, 1)
    blue = (frame * 17) % 256

    def pixel(x, y):
        return (int(x * 255 / w), int(y * 255 / h), blue)

    return pixel


GENERATORS = {
    "solid": _solid,
    "checkerboard": _checkerboard,
    "gradient": _gradient,
}


def write_bmp(path, width, height, pixel_fn):
    """Write a 24-bit uncompressed BMP using only struct."""
    row_size = ((width * 3 + 3) // 4) * 4  # pad each row to a 4-byte boundary
    pixel_size = row_size * height
    file_size = PIXEL_OFFSET + pixel_size

    with open(path, "wb") as f:
        # BITMAPFILEHEADER
        f.write(b"BM")
        f.write(struct.pack("<IHHI", file_size, 0, 0, PIXEL_OFFSET))
        # BITMAPINFOHEADER (BI_RGB, 24bpp, 2835 ppm ~= 72 DPI)
        f.write(
            struct.pack(
                "<IiiHHIIiiII",
                INFO_HEADER_SIZE,
                width,
                height,
                1,  # planes
                24,  # bits per pixel
                0,  # compression = BI_RGB
                pixel_size,
                2835,
                2835,
                0,  # colors used
                0,  # important colors
            )
        )
        # Pixel data: bottom-up rows, BGR order, padded.
        for y in range(height - 1, -1, -1):
            row = bytearray()
            for x in range(width):
                r, g, b = pixel_fn(x, y)
                row.extend((b, g, r))
            if len(row) < row_size:
                row.extend(b"\x00" * (row_size - len(row)))
            f.write(row)


def parse_args():
    parser = argparse.ArgumentParser(
        description="Generate synthetic solid/checkerboard/gradient BMP frames for encode tests."
    )
    parser.add_argument("--width", type=int, default=1920, help="image width in pixels (default: 1920)")
    parser.add_argument("--height", type=int, default=1080, help="image height in pixels (default: 1080)")
    parser.add_argument("--count", type=int, default=3, help="number of frames (default: 3)")
    parser.add_argument("--out", default="frames", help="output directory (default: frames)")
    parser.add_argument(
        "--pattern",
        choices=["solid", "checkerboard", "gradient", "all"],
        default="all",
        help="pattern to generate (default: all, cycling through the three)",
    )
    return parser.parse_args()


def main():
    args = parse_args()
    if args.width <= 0 or args.height <= 0 or args.count <= 0:
        raise SystemExit("error: --width, --height and --count must be positive")

    os.makedirs(args.out, exist_ok=True)
    patterns = list(GENERATORS) if args.pattern == "all" else [args.pattern]

    for i in range(args.count):
        name = patterns[i % len(patterns)]
        pixel_fn = GENERATORS[name](args.width, args.height, i)
        filename = "frame_{:04d}_{}.bmp".format(i, name)
        path = os.path.join(args.out, filename)
        write_bmp(path, args.width, args.height, pixel_fn)
        print(path)


if __name__ == "__main__":
    main()
