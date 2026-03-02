# Release v0.6.3-rc.1

## 🐛 Bug Fixes

### Kline API Response Parsing
- **Issue**: Kline command was not working due to incorrect API response parsing
- **Root Cause**: API returns `{s, t[], o[], h[], l[], c[], v[]}` format instead of array of objects
- **Fix**: 
  - Added `KlineResponse` struct to handle the API response format
  - Implemented conversion from `KlineResponse` to `Vec<Kline>` for consistent interface
  - Fixes ISSUE-2.1

## 📋 Files Changed
- `src/client/mod.rs` - Updated kline fetching logic
- `src/models.rs` - Added KlineResponse struct

## 🔧 Version Updates
- Cargo.toml: `0.6.2` → `0.6.3-rc.1`
- version.json: `0.6.2` → `0.6.3-rc.1`
- CHANGELOG.md: Added v0.6.3-rc.1 section
- SKILL.md: Updated version reference

## 🧪 Testing
This is a release candidate. Please test the following:
1. `standx market kline BTC-USD -r 60 --from 1d` - Verify kline data displays correctly
2. `standx market kline ETH-USD -r 1D --from 7d` - Test different time ranges
3. Verify other market commands still work: `ticker`, `depth`, `symbols`

## 📥 Installation

### Linux
```bash
curl -L -o /tmp/standx.tar.gz https://github.com/wjllance/standx-cli/releases/download/v0.6.3-rc.1/standx-v0.6.3-rc.1-x86_64-unknown-linux-gnu.tar.gz
tar -xzf /tmp/standx.tar.gz -C /tmp
sudo mv /tmp/standx /usr/local/bin/
sudo chmod +x /usr/local/bin/standx
```

### macOS (Apple Silicon)
```bash
curl -L -o /tmp/standx.tar.gz https://github.com/wjllance/standx-cli/releases/download/v0.6.3-rc.1/standx-v0.6.3-rc.1-aarch64-apple-darwin.tar.gz
tar -xzf /tmp/standx.tar.gz -C /tmp
sudo mv /tmp/standx /usr/local/bin/
sudo chmod +x /usr/local/bin/standx
```

---

**Full Changelog**: https://github.com/wjllance/standx-cli/blob/main/CHANGELOG.md
