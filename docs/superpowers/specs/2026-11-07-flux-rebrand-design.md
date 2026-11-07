# FLUX rebrand

## Direction

The product's public name is FLUX, always uppercase. Audio playback, MIDI/OSC cues, multiple timelines and Live operation have expanded its role beyond a timecode generator. Timecode remains the shared master clock at the centre of the product.

## Scope

Update the English and Spanish catalogues, window and HTML titles, native bundle display name, file-association descriptions, diagnostic report heading and MIDI client display names. Pair the existing modular F with a readable FLUX wordmark in the app. Preserve the instrument styling and current app icon. Update the product and design documents to record the name and compatibility boundary.

## Compatibility

Keep the application identifier `com.fluxtimecode.app`, package and crate names, settings and storage keys, `.fxtc` format and MIME type, asset filenames and `/flux/...` OSC addresses. No project migration is required. MIDI client display names change to FLUX; external tools that remember those display names may need to select them again.

## Validation

Run the frontend typecheck, unit suite and production build, plus Rust formatting checks. Inspect the launcher, welcome window and editor brand mark at their supported window sizes. Search for obsolete public names while preserving technical identifiers.

## Project extension transition

The subsequent extension change makes `.flx` the default for new projects and Save as. Open accepts both `.flx` and `.fxtc`; Save preserves an existing project's path and extension. Both extensions use the same ZIP format and version. Register both file associations, with `.flx` using `application/x-flx` and legacy `.fxtc` retaining `application/x-fxtc`. Recognise both extensions case-insensitively for command-line opening and project deletion. Existing recent-project entries and recovery paths remain unchanged. Verify archive round trips for both extensions and Save as from a legacy project without modifying its source file.
