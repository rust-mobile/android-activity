# Changelog
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]
### Added
- Emit an `InputAvailable` event for new input with `NativeActivity` and `GameActivity`
  enabling gui apps that don't render continuously
- Oboe and Cpal audio examples added
- `AndroidApp` is now `Send` + `Sync`
### Changed
- *Breaking*: updates to `ndk 0.7` and `ndk-sys 0.4`
- *Breaking*: `AndroidApp::config()` now returns a clonable `ConfigurationRef` instead of a deep `Configuration` copy

## [0.1.1] - 2022-07-04
### Changed
- Documentation fixes

## [0.1] - 2022-07-04
### Added
- Initial release