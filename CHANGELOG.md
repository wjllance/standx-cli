# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.5.0] - 2026-02-27

### Added
- OpenClaw Skill configuration (#47)
  - Added `openclaw/SKILL.md` with security-focused authentication guide
  - Added `openclaw/skill.json` with credential declarations
  - Added `.clawhub/` for ClawHub packaging
- GitHub PR Review skill (#59)
  - Extracted PR review workflow into reusable skill
  - Added SKILL.md with workflow definition
  - Added best practices and key learnings

### Security
- Prioritize environment variables over command-line args
- Add security warnings for unsafe practices (shell history leakage)
- Add security checklist for pre-installation verification (#55)

### Tests
- Add Ed25519 Signer tests (#57)
- Add Client mock tests (#58)
- Add JWT Token tests (#56)
- Improve Auth Credentials tests (#53)

### Documentation
- Update authentication section with 4 login methods
- Add permission requirements table
- Add comprehensive docs/ directory with 11 guides

## [0.4.2] - 2026-02-26

### Fixed
- Position model updated (PR #24)
- Splash screen version (PR #23)

## [0.4.0] - 2026-02-26

### Added
- Telemetry module (PR #19)
- Improved authentication flow
- Splash screen improvements

## [0.3.6] - 2026-02-26

### Documentation
- Improved README authentication section

## [0.3.5] - 2026-02-26

### Changed
- OpenClaw Skill improvements
- Fixed GitHub Release binary upload in CI workflow
