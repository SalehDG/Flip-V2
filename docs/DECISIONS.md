# Flip V2 — Development decisions

## Attachment format policy (confirmed)

- Accept PNG and JPEG.
- Determine actual format from magic bytes, not filename extension or declared MIME type.
- Preserve original bytes; do not convert JPEG to PNG.
- Persist declared MIME type and detected format separately.
- Use an extension based on detected format.
- Unknown/corrupt formats are not accepted as image attachments and must be handled by later validation as Needs Review.

Evidence: the two supplied `.eml` examples declare `image/png` and use `.png` filenames, but their attachment bytes begin with the JPEG signature `FF D8 FF`.
