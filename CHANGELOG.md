# Changelog

Release entries supply the corresponding GitHub Release notes. When an entry is
missing, publication generates it from the tagged Git history.

## [0.1.0-alpha.1](https://github.com/sailry/sailry-harness/releases/tag/v0.1.0-alpha.1)

The first Sailry Harness alpha brings AI conversations and development tools into
a native workspace. Features and plugin APIs may change during alpha development.

### Highlights

- A macOS workspace with projects, conversations, files, diffs and terminals
- Local execution inside Desktop and persistent tasks on your own Sailry Host
- Plan review, guidance during execution, and delegated agents with configurable roles
- Plugins for code, research, documents, SSH and databases, with an extensible SDK
- Multiple model providers, compatible endpoints, and ChatGPT and Claude sign-in
- Document previews, conversation exports, themes, and optional speech, OCR and embedding models
- Ad-hoc macOS downloads first, followed by independent formal signing and notarization, plus Host packages for macOS and Linux

### Fixed

- Link XZ support statically so macOS packages do not depend on a developer's Homebrew libraries
- Keep background inference workers compatible with Linux systems using glibc 2.28
- Retry interrupted Apple ticket validation and recover verified successful builds during publication

### Changed

- Discover signed Desktop updates from exact-version Release catalogs and require matching Desktop and Host versions, with profile-preserving Host updates
- Reduce repeated Node and package setup in integration tests while preserving real service coverage
- Share dependency sources and compiler caches across validation and release builds within the repository cache budget

### Downloads

- Desktop: macOS 13 or later, Apple silicon and Intel
- Host: macOS and Linux, ARM64 and x64
