# Picture fixtures

Four quadrants — red, green, blue, yellow — 64x64, in each raster format this
backend decodes (`RND-12`).

**Encoded by tools that are not this repository**, deliberately: `sips`
(Apple's ImageIO) for JPEG, GIF, BMP and TIFF, and `cwebp` (Google's reference
encoder, lossless) for WebP. A fixture produced by the same crate that decodes
it proves the crate agrees with itself, which is the failure this project keeps
finding. These prove the decoder reads bitstreams somebody else wrote.

The quadrants are the assertion: a decoder that returned the right dimensions
and garbage pixels, or that flipped the image vertically — the classic BMP
bug — passes "it decoded" and fails these.

JPEG is lossy, so its test allows a wide tolerance. Every other format here is
lossless and is asserted exactly.
