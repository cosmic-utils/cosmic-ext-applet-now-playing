# Changelog

All notable changes to this project are documented in this file.

## [Unreleased]

### Fixed

- Album art now shows for players that publish remote cover URLs, such as Spotify (#12).
- Album art also loads from inline `data:` URLs and from local files without an extension.
- Covers are loaded in the background, so slow disks or networks no longer block the panel.

### Changed

- The Flatpak can now access the network and `/tmp` (read-only) to load album art.

## [0.2.1] - 2026-09-01

### Added

- Configurable Previous, Play/Pause, and Next controls in the panel.
- Czech localization.

### Changed

- Replaced frequent media polling with MPRIS D-Bus events for more responsive playback updates.
- Expanded localized app metadata.

## [0.2.0] - 2026-07-28

### Added

- Selection between multiple active media players.
- Play/pause control directly from the media icon in the panel.
- A setting to enable or disable album-inspired panel colors.
- Native frosted-glass blur for the applet popup.
- Spanish localization.

### Changed

- Improved media metadata, album artwork, and player-state handling.
- Updated the existing translations and accessibility labels.
- Updated libcosmic and the Rust dependencies.

## [0.1.0] - 2026-07-15

- Initial release.

[0.2.1]: https://github.com/cosmic-utils/cosmic-ext-applet-now-playing/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/cosmic-utils/cosmic-ext-applet-now-playing/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/cosmic-utils/cosmic-ext-applet-now-playing/releases/tag/v0.1.0
