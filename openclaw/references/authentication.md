# Authentication Details

## JWT Token

The JWT token is the primary authentication method for StandX CLI.

### Obtaining a Token

1. Visit https://standx.com/user/session
2. Generate a new JWT token
3. Copy the token value

### Token Properties

- **Validity**: 7 days
- **Scope**: Read account data, place/cancel orders
- **Storage**: Environment variable recommended

### Environment Variable Setup

```bash
# Add to ~/.bashrc or ~/.zshrc
export STANDX_JWT="your_jwt_token_here"

# Reload configuration
source ~/.bashrc
```

## Private Key (Optional)

Required for certain trading operations.

### Format

- Algorithm: Ed25519
- Encoding: Base58

### Setup

```bash
export STANDX_PRIVATE_KEY="your_base58_private_key"
```

## Security Best Practices

1. **Never commit credentials to git**
2. **Use environment variables** (not command-line arguments)
3. **Set file permissions** to 600 for credential files
4. **Rotate tokens regularly** (every 7 days)
5. **Use separate tokens** for different environments

## Troubleshooting Authentication

### "Unauthorized" Error

- Check if token is expired
- Verify `STANDX_JWT` is set correctly
- Run `standx auth status` to diagnose

### "Permission Denied" Error

- Some operations require private key
- Verify `STANDX_PRIVATE_KEY` is set for trading
