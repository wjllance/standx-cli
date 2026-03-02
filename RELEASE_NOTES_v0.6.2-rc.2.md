# Release v0.6.2-rc.2

## 🐛 Bug Fixes

### Trade Model Field Mapping (#113)
- Correct Trade model field mapping for proper decoding
- Fix trade history display issues

## 📚 Documentation

### README Portfolio Command (#115)
- Add Portfolio command documentation to README
- Include usage examples and options

## 📋 Summary
This is release candidate 2 for v0.6.2 testing.

**Changes since v0.6.1:**
- Trade model field mapping fix
- Portfolio command documentation

## 🧪 Testing Checklist
- [ ] `standx --version` should show `0.6.2-rc.2`
- [ ] `standx trade history BTC-USD` - Verify trade history displays correctly
- [ ] `standx portfolio snapshot` - Test portfolio command
- [ ] `standx dashboard snapshot` - Verify dashboard works

## 📥 Installation

### Linux
```bash
curl -L -o /tmp/standx.tar.gz https://github.com/wjllance/standx-cli/releases/download/v0.6.2-rc.2/standx-v0.6.2-rc.2-x86_64-unknown-linux-gnu.tar.gz
tar -xzf /tmp/standx.tar.gz -C /tmp
sudo mv /tmp/standx /usr/local/bin/
sudo chmod +x /usr/local/bin/standx
```

### macOS (Apple Silicon)
```bash
curl -L -o /tmp/standx.tar.gz https://github.com/wjllance/standx-cli/releases/download/v0.6.2-rc.2/standx-v0.6.2-rc.2-aarch64-apple-darwin.tar.gz
tar -xzf /tmp/standx.tar.gz -C /tmp
sudo mv /tmp/standx /usr/local/bin/
sudo chmod +x /usr/local/bin/standx
```

---

⚠️ **This is a pre-release version for testing purposes.**

**Full Changelog**: https://github.com/wjllance/standx-cli/blob/main/CHANGELOG.md
