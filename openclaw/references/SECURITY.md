# Security Checklist for standx-cli

## Before Installing

### 1. Inspect the Homebrew Formula

Verify the formula downloads from expected sources:

```bash
# View the formula
curl -sL https://raw.githubusercontent.com/wjllance/homebrew-standx-cli/main/standx-cli.rb

# Confirm it points to official GitHub releases
# Expected: url "https://github.com/wjllance/standx-cli/releases/download/..."
```

**Check for:**
- ✅ Download URL matches `github.com/wjllance/standx-cli/releases`
- ✅ SHA256 checksum is present and valid
- ✅ No suspicious external URLs

### 2. Verify Binary Artifacts

Before installing, verify the binary checksum:

```bash
# Download binary and checksums
curl -L -o standx.tar.gz https://github.com/wjllance/standx-cli/releases/download/v0.4.2/standx-v0.4.2-x86_64-unknown-linux-gnu.tar.gz
curl -L -o checksums.txt https://github.com/wjllance/standx-cli/releases/download/v0.4.2/checksums.txt

# Verify
sha256sum -c checksums.txt --ignore-missing
```

### 3. Resolve Metadata Mismatch

**Known Issue:** ClawHub registry shows "no required credentials" but SKILL.md requires `STANDX_JWT`.

**Resolution:**
- Trust the SKILL.md/skill.json metadata (they declare `STANDX_JWT` as primary credential)
- This is a ClawHub registry display issue, not a security risk
- Publisher has been notified to fix registry metadata

### 4. Protect Your Credentials

**Never:**
- ❌ Paste private keys or tokens into shell commands
- ❌ Commit credentials to Git/VCS
- ❌ Share credentials in chat or email

**Always:**
- ✅ Use environment variables
- ✅ Use a secure secrets manager
- ✅ Set file permissions to 600 for credential files

```bash
# Good: Environment variables
export STANDX_JWT="your_token"
export STANDX_PRIVATE_KEY="your_key"

# Bad: Command line arguments (leaks to shell history)
standx auth login --token "your_token" --private-key "your_key"
```

### 5. Test Before Trading

Because this is a **trading tool**:

1. **Start with read-only commands:**
   ```bash
   standx market ticker BTC-USD    # No auth needed
   standx account balances         # Read-only
   ```

2. **Use sandbox/test accounts first**

3. **Verify binary legitimacy:**
   - Check GitHub repo: https://github.com/wjllance/standx-cli
   - Review release artifacts
   - Confirm tap ownership

4. **Only then authorize with real funds**

## Higher Assurance Steps

For maximum security:

1. **Review the GitHub repository:**
   - https://github.com/wjllance/standx-cli
   - Check source code
   - Review CI/CD workflows

2. **Build from source:**
   ```bash
   git clone https://github.com/wjllance/standx-cli.git
   cd standx-cli
   cargo build --release
   ```

3. **Use ClawHub installation (recommended):**
   ```bash
   clawhub install standx-cli
   ```

## Red Flags to Watch For

⚠️ **Stop and verify if you see:**
- Download URLs not from `github.com/wjllance/standx-cli`
- Missing SHA256 checksums
- Formula pointing to external/unverified sources
- Requests for credentials in unexpected places

## Reporting Issues

If you discover security concerns:
- GitHub Issues: https://github.com/wjllance/standx-cli/issues
- Homebrew Tap Issues: https://github.com/wjllance/homebrew-standx-cli/issues
