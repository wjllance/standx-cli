# Release v0.6.3-rc.3

## 🐛 Bug Fixes

### Market Trades API Decoding (#143)
- Resolve trades API response decoding error
- Fix trade history data parsing issues
- Ensure proper handling of trades endpoint response format

### Market Depth Table Alignment (#144)
- Fix output table formatting alignment issues
- Improve depth display readability
- Better column spacing in market depth view

### Zero Quantity Positions (#140)
- Filter out zero-quantity positions from portfolio display
- Cleaner portfolio view without empty positions
- Reduce visual noise in position listings

### Quiet Mode Flag (#141)
- Properly handle `-q` (quiet) flag across all commands
- Suppress non-essential output when quiet mode is enabled
- Consistent quiet mode behavior throughout CLI

### Test Environment (#142)
- Resolve test_from_env failure in CI
- Improve test stability across different environments
- Better test isolation and cleanup

## 📋 Summary
This is release candidate 3 for v0.6.3 testing.

**Changes since v0.6.3-rc.2:**
- Fixed trades API decoding issues
- Improved market depth table formatting
- Filtered zero-quantity positions
- Fixed quiet mode flag handling
- Enhanced test stability

## 🧪 Testing Checklist
- [ ] `standx --version` should show `0.6.3-rc.3`
- [ ] `standx market trades BTCUSDT` works correctly
- [ ] `standx depth BTCUSDT` shows properly aligned table
- [ ] `standx portfolio snapshot` filters zero positions
- [ ] `standx -q market ticker` suppresses non-essential output
- [ ] All CI tests pass

## 📥 Installation

### Linux
```bash
curl -L -o /tmp/standx.tar.gz https://github.com/wjllance/standx-cli/releases/download/v0.6.3-rc.3/standx-v0.6.3-rc.3-x86_64-unknown-linux-gnu.tar.gz
tar -xzf /tmp/standx.tar.gz -C /tmp
sudo mv /tmp/standx /usr/local/bin/
sudo chmod +x /usr/local/bin/standx
```

### macOS (Apple Silicon)
```bash
curl -L -o /tmp/standx.tar.gz https://github.com/wjllance/standx-cli/releases/download/v0.6.3-rc.3/standx-v0.6.3-rc.3-aarch64-apple-darwin.tar.gz
tar -xzf /tmp/standx.tar.gz -C /tmp
sudo mv /tmp/standx /usr/local/bin/
sudo chmod +x /usr/local/bin/standx
```

---

⚠️ **This is a pre-release version for testing purposes.**

**Full Changelog**: https://github.com/wjllance/standx-cli/blob/main/CHANGELOG.md
