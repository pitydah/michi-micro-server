# Licensing

Michi Micro Server is licensed under **GPL-3.0-only**.

## Compatibility

| License | Compatible | Notes |
|---------|------------|-------|
| GPL-3.0-only | ✅ Same license | Navidrome, Snapcast (inspiration only, no code copied) |
| Apache-2.0 | ✅ Compatible with attribution | OpenSubsonic, LocalSend, Music Assistant |
| MIT | ✅ Compatible with copyright notice | |
| GPL-2.0-only | ❌ Avoid | Jellyfin, MPD — only if "or later" clause exists |
| AGPL-3.0 | ❌ Avoid | Ampache, Funkwhale, Tempo |

## Rules

1. No AGPL code is copied into this repository.
2. No GPL-2.0-only code is copied without explicit "or later" compatibility.
3. Any copied code (MIT, Apache-2.0, GPL-3.0 compatible) must be declared in `THIRD_PARTY_NOTICES.md` with:
   - File name and path
   - Source URL
   - License
   - Author/Copyright holder
   - Changes made
   - Reason for inclusion
4. When only implementing specifications or taking inspiration from external projects, this is stated in `docs/inspirations.md` and `THIRD_PARTY_NOTICES.md`.
5. No logos, branding, distinctive UI, images, or trademarks from other projects are used.

## Current Third-Party Inspirations

See `THIRD_PARTY_NOTICES.md` for detailed declarations.
