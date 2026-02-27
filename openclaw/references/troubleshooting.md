# Troubleshooting

## Common Issues

### Command not found: standx

**Problem**: `standx` command not recognized

**Solutions**:
1. Verify installation: `which standx`
2. Check PATH includes `/usr/local/bin`
3. Reinstall using `clawhub install standx-cli`

### Authentication Failed

**Problem**: "Unauthorized" or "Invalid token" errors

**Solutions**:
1. Check token expiration: `standx auth status`
2. Verify `STANDX_JWT` is set: `echo $STANDX_JWT`
3. Regenerate token at https://standx.com/user/session
4. Reload shell config: `source ~/.bashrc`

### Rate Limit Exceeded

**Problem**: "429 Too Many Requests"

**Solutions**:
1. Reduce request frequency
2. Use WebSocket streams for real-time data
3. Cache responses when appropriate

### Connection Timeout

**Problem**: Network-related errors

**Solutions**:
1. Check internet connection
2. Verify API endpoint: `standx config get base_url`
3. Check StandX status page

## Security Best Practices

### Verify Download Integrity

All releases include SHA256 checksums. Always verify before installing:

```bash
# Download binary and checksums
curl -L -o /tmp/standx.tar.gz https://github.com/wjllance/standx-cli/releases/download/v0.4.4/standx-v0.4.4-x86_64-unknown-linux-gnu.tar.gz
curl -L -o /tmp/checksums.txt https://github.com/wjllance/standx-cli/releases/download/v0.4.4/checksums.txt

# Verify checksum
cd /tmp && sha256sum -c checksums.txt --ignore-missing
```

### Avoid Pipe-to-Shell

⚠️ **Warning**: Never run `curl ... | sh` without inspecting the script first. Always:
1. Download the script: `curl -L -o install.sh ...`
2. Inspect the content: `cat install.sh`
3. Then execute: `sh install.sh`

### Recommended Installation Methods (Most Secure)

1. **ClawHub**: `clawhub install standx-cli` ✅ Recommended
2. **Homebrew**: `brew install standx-cli` ✅ Recommended
3. **Direct Download with checksum verification** ⚠️ Requires manual verification

## Debug Mode

Enable verbose output:

```bash
standx -v market ticker BTC-USD
```

## Getting Help

1. Check command help: `standx --help`
2. Check subcommand help: `standx market --help`
3. Visit GitHub Issues: https://github.com/wjllance/standx-cli/issues

## Reporting Bugs

When reporting issues, include:

1. StandX CLI version: `standx --version`
2. Operating system
3. Command that failed
4. Error message (with `-v` flag output)
5. Steps to reproduce
