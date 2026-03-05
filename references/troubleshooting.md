# StandX CLI Troubleshooting

## Installation Issues

### Command not found

```bash
# Check if installed
which standx

# If not found, try:
# 1. Homebrew
brew tap wjllance/standx-cli
brew install standx-cli

# 2. Direct download (Linux)
curl -L -o standx.tar.gz https://github.com/wjllance/standx-cli/releases/latest/download/standx-linux-x86_64.tar.gz
tar -xzf standx.tar.gz
sudo mv standx /usr/local/bin/

# 3. Direct download (macOS)
curl -L -o standx.tar.gz https://github.com/wjllance/standx-cli/releases/latest/download/standx-macos-aarch64.tar.gz
tar -xzf standx.tar.gz
sudo mv standx /usr/local/bin/
```

## Authentication Issues

### "Not authenticated" error

**Solution:** Configure environment variables or login

```bash
# Recommended: Environment variables
export STANDX_JWT="your_jwt_token"
export STANDX_PRIVATE_KEY="your_private_key"

# Verify
standx auth status
```

### Environment variables not working

**Check if variables are set:**

```bash
echo $STANDX_JWT
echo $STANDX_PRIVATE_KEY
```

**Common causes:**

1. **Shell not reloaded after editing rc file**
   ```bash
   source ~/.bashrc  # or ~/.zshrc
   ```

2. **Wrong variable name**
   - Correct: `STANDX_JWT`
   - Wrong: `STANDX_TOKEN`, `JWT_TOKEN`

3. **Running in subshell or script**
   - Variables must be exported: `export VAR=value`
   - Check with `env | grep STANDX`

4. **Running via sudo**
   - Environment variables are stripped by sudo
   - Use `sudo -E` or configure in root's environment

### "Token expired" error

**Solution:** Get new token from https://standx.com/user/session and update environment variable

```bash
# Update in shell config
export STANDX_JWT="new_token"

# Reload
source ~/.bashrc

# Verify
standx auth status
```

### "Private key required" error

**Solution:** Trading operations require Ed25519 private key

```bash
# Add private key to environment
export STANDX_PRIVATE_KEY="your_private_key"

# Or login interactively
standx auth login --interactive
```

### Credentials visible in shell history

**Problem:** You used `standx auth login --token "xxx" --private-key "yyy"`

**Solution:**

1. **Clear history immediately:**
   ```bash
   history -d $(history 1)
   # or clear all
   history -c
   ```

2. **Switch to environment variables:**
   ```bash
   # Add to ~/.bashrc or ~/.zshrc
   export STANDX_JWT="your_token"
   export STANDX_PRIVATE_KEY="your_key"
   ```

3. **Use file-based login for scripts:**
   ```bash
   standx auth login --token-file ~/.standx_token --key-file ~/.standx_key
   ```

### Permission denied on credential files

**Solution:** Set proper permissions

```bash
chmod 600 ~/.standx_token ~/.standx_key
chmod 700 ~/.config/standx
```

## API Issues

### "HTTP request failed" error

**Possible causes:**

1. Network connectivity
2. API endpoint unavailable
3. Rate limiting

**Solutions:**

```bash
# Check network
ping perps.standx.com

# Check API status
curl https://perps.standx.com/api/query_symbol_info

# Wait and retry
sleep 5 && standx market ticker BTC-USD
```

### "Symbol not found" error

**Solution:** Check available symbols

```bash
standx market symbols
```

### K-line "no_data" response

**Cause:** No data available for the requested time range

**Solution:** Try different time range

```bash
# Use relative time
standx market kline BTC-USD -r 60 --from 1d

# Or use limit instead of from/to
standx market kline BTC-USD -r 60 -l 10
```

## WebSocket Issues

### "invalid token" for user streams

**Status:** Known issue [#3](https://github.com/wjllance/standx-cli/issues/3)

**Workaround:** Use public streams or check token validity

```bash
# Public streams work without auth
standx stream price BTC-USD
standx stream depth BTC-USD
standx stream trade BTC-USD
```

### Connection drops

**Solution:** CLI auto-reconnects, no action needed. For debugging:

```bash
standx -v stream price BTC-USD
```

## Output Issues

### JSON parsing errors

**Solution:** Check if output is valid JSON

```bash
standx -o json market ticker BTC-USD | jq .
```

### CSV format issues

**Solution:** Ensure proper encoding

```bash
export LANG=en_US.UTF-8
standx -o csv market symbols
```

## Performance Issues

### Slow response

**Possible causes:**

1. Network latency
2. API rate limiting
3. Large data queries

**Solutions:**

```bash
# Use limit to reduce data
standx market kline BTC-USD -r 60 --from 1d -l 10

# Use quiet mode for faster output
standx -o quiet market ticker BTC-USD
```

## Security Best Practices

### Credential Storage

| Method | Security | Use Case |
|--------|----------|----------|
| Environment variables | ⭐⭐⭐ High | Development, interactive use |
| File with 600 permissions | ⭐⭐⭐ High | Automation scripts |
| Command line args | ⭐ Low (leaks to history) | Avoid in production |

### Recommended Setup

```bash
# 1. Create secure directory
mkdir -p ~/.config/standx
chmod 700 ~/.config/standx

# 2. Add to shell config (~/.bashrc or ~/.zshrc)
export STANDX_JWT="your_jwt_token"
export STANDX_PRIVATE_KEY="your_private_key"

# 3. Reload
source ~/.bashrc

# 4. Verify
standx auth status
```

### Rotating Credentials

JWT tokens expire after 7 days. Set a reminder to rotate:

```bash
# Check expiration
standx auth status

# When expired, get new token and update environment
export STANDX_JWT="new_token"
```

## Getting Help

### Check version

```bash
standx --version
```

### View help

```bash
standx --help
standx market --help
standx order create --help
```

### GitHub Issues

https://github.com/wjllance/standx-cli/issues

### Documentation

https://github.com/wjllance/standx-cli/tree/main/docs
