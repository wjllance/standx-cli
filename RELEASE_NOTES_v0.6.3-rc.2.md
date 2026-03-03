# Release v0.6.3-rc.2

## ✨ New Features

### Command Short Aliases (#137)
- Add short aliases for common commands
- Examples: `s` for `snapshot`, `w` for `watch`, `d` for `dashboard`
- Improve CLI usability and typing efficiency

## 🐛 Bug Fixes

### Kline Timestamp Format (#129)
- Format Unix timestamps to human-readable time
- Display format: `YYYY-MM-DD HH:MM:SS`
- Improve readability of kline/candlestick data

### Depth Spread Display (#138)
- Show bid-ask spread in both dollar amount and percentage
- Better market depth visualization
- Help traders quickly assess market liquidity

### WebSocket Debug Logs (#139)
- Ensure debug logs only show with `--verbose` flag
- Clean up watch mode output
- Remove unwanted DEBUG messages in normal operation

## 📋 Summary
This is release candidate 2 for v0.6.3 testing.

**Changes since v0.6.3-rc.1:**
- Command short aliases for improved UX
- Better timestamp formatting in kline output
- Enhanced depth spread display
- Cleaner watch mode output (no debug logs)

## 🧪 Testing Checklist
- [ ] `standx --version` should show `0.6.3-rc.2`
- [ ] `standx market ticker -s BTCUSDT` works with short flags
- [ ] `standx kline BTCUSDT` shows readable timestamps
- [ ] `standx depth BTCUSDT` shows spread in $ and %
- [ ] `standx dashboard --watch` runs without DEBUG logs
- [ ] Command aliases work (e.g., `standx d s` for dashboard snapshot)

## 📥 Installation

### Linux
```bash
curl -L -o /tmp/standx.tar.gz https://github.com/wjllance/standx-cli/releases/download/v0.6.3-rc.2/standx-v0.6.3-rc.2-x86_64-unknown-linux-gnu.tar.gz
tar -xzf /tmp/standx.tar.gz -C /tmp
sudo mv /tmp/standx /usr/local/bin/
sudo chmod +x /usr/local/bin/standx
```

### macOS (Apple Silicon)
```bash
curl -L -o /tmp/standx.tar.gz https://github.com/wjllance/standx-cli/releases/download/v0.6.3-rc.2/standx-v0.6.3-rc.2-aarch64-apple-darwin.tar.gz
tar -xzf /tmp/standx.tar.gz -C /tmp
sudo mv /tmp/standx /usr/local/bin/
sudo chmod +x /usr/local/bin/standx
```

---

⚠️ **This is a pre-release version for testing purposes.**

**Full Changelog**: https://github.com/wjllance/standx-cli/blob/main/CHANGELOG.md
