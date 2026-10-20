# gm8-font

Bounded `no_std` parser for compressed GameMaker 8.0 Font records. It decodes
the asset and retains the 256-slot glyph metric map plus the raw alpha atlas;
it does not call system font APIs or upload textures to a renderer.

The table loader first validates the full GM8.0 resource directory and then
decodes only its Font records. Per-record decompression, names, dimensions,
atlas bytes, record count, and total decoded atlas bytes have explicit limits.
Host tests use synthetic records only.

The Switch app uses this crate in its GM8 resource diagnostic. The renderer and
OpenGMK game loop do not consume the returned fonts yet.
