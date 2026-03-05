# Release Notes v0.7.0

## 🚀 Dashboard MVP & Automated Releases

We're excited to announce the release of StandX CLI v0.7.0, featuring the complete Dashboard MVP and automated release workflows!

---

## ✨ What's New

### Dashboard MVP (#157)

The dashboard has been completely rebuilt with production-ready features:

- **Comfy-table Formatting**: Beautiful, aligned tables for better readability
- **Real-time Order Book Depth**: Visualize market depth directly in the dashboard
- **Recent Trades Panel**: See live BUY/SELL activity with color coding
- **Local Timezone Support**: All timestamps displayed in your local timezone
- **Graceful Exit**: Press Ctrl+C to exit watch mode cleanly
- **Instant Refresh**: Data is fetched before clearing screen to reduce flicker
- **Version in Title**: Dashboard title now shows the current version

### Command Short Aliases (#137)

Faster typing with short command aliases:

| Full Command | Short Alias |
|--------------|-------------|
| `standx market ticker` | `standx m t` |
| `standx market depth` | `standx m d` |
| `standx portfolio snapshot` | `standx p s` |
| `standx dashboard --watch` | `standx d -w` |

### Automated Release Workflow (#167)

- **Push Tag → Auto Release**: Simply push a tag to trigger the release process
- **Multi-platform Builds**: Automatic builds for macOS ARM64, Linux x86_64, and Linux ARM64
- **Pre-release Support**: RC, Beta, and Alpha versions are automatically marked as pre-release
- **Homebrew Integration**: Stable releases automatically update the Homebrew formula

---

## 🐛 Bug Fixes

- Fixed dashboard and portfolio command handling
- Enhanced trade handling and output formatting
- Removed duplicate tests module
- Fixed test version hardcoding issues

---

## 📋 Installation

### Homebrew

```bash
brew tap wjllance/standx-cli
brew install standx-cli
```

### Binary Download

Download pre-built binaries from [GitHub Releases](https://github.com/wjllance/standx-cli/releases/tag/v0.7.0).

---

## 🔄 Migration Guide

No breaking changes. All existing commands continue to work as before.

---

## 📚 Documentation

- [Full Documentation](https://github.com/wjllance/standx-cli/tree/main/docs)
- [Changelog](https://github.com/wjllance/standx-cli/blob/main/CHANGELOG.md)

---

## 🙏 Contributors

Thanks to all contributors who made this release possible!

---

*Released: 2026-03-05*  
*Version: v0.7.0*
