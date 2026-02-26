# OpenClaw Integration

This directory contains OpenClaw skill configuration for StandX CLI.

## Files

- `SKILL.md` - OpenClaw skill documentation
- `skill.json` - OpenClaw metadata (credentials, install methods)

## Quick Start for OpenClaw Users

```bash
# Install via ClawHub
clawhub install standx-cli

# Configure credentials
export STANDX_JWT="your_jwt_token"
export STANDX_PRIVATE_KEY="your_private_key"

# Start trading
standx market ticker BTC-USD
```

See [SKILL.md](SKILL.md) for complete documentation.
