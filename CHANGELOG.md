# Changelog

## v0.1.1 — 2026-11-01

- Optimize FFT stages and real transforms while preserving numerical results.

## v0.1.0 — 2026-11-01

- Add the Rust builder and Darwin ABI adapters for `dee_dthd_encoder` on Linux.
- Add payload screening, pinned inputs and two-build verification.
- Allow unpinned encoder payloads with `--allow-unpinned-payload` after screening.
- Build natively on Ubuntu 24.04 with GCC 13 and LLVM 18; download dependencies
  from pinned URLs with SHA-256 verification.
- Derive help versions from release tags, with an abbreviated commit ID and
  a commit count when ahead of the tag.
