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
