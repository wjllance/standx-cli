# Release v0.6.3-rc.1

## 🐛 Bug Fixes

### Auth Non-TTY Support (#127)
- Support non-TTY environments for login
- Fix authentication issues in CI/automated environments
- Improved error handling for headless environments

### Dashboard+Portfolio Auth Handling (#125)
- Properly handle AuthRequired error for anonymous mode
- Improve error messages for unauthenticated users
- Better UX when accessing dashboard/portfolio without login

## 📋 Summary
This is release candidate 1 for v0.6.3 testing.

**Changes since v0.6.2:**
- Non-TTY environment support for authentication
- Better auth error handling in dashboard and portfolio commands

## 🧪 Testing Checklist
- [ ] `standx --version` should show `0.6.3-rc.1`
- [ ] `standx auth login` works in non-TTY environment
- [ ] `standx dashboard snapshot` shows proper auth error when not logged in
- [ ] `standx portfolio snapshot` shows proper auth error when not logged in

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

⚠️ **This is a pre-release version for testing purposes.**

**Full Changelog**: https://github.com/wjllance/standx-cli/blob/main/CHANGELOG.md
