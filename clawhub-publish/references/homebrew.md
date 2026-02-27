# Homebrew Installation Security

## Third-Party Tap Notice

The Homebrew installation uses a third-party tap (`wjllance/standx-cli`). While this is a common practice, you should verify the source before installing.

## Verification Steps

### 1. Inspect the Formula

Before installing, you can view the formula source:

```bash
# View the formula directly
curl -sL https://raw.githubusercontent.com/wjllance/homebrew-standx-cli/main/standx-cli.rb

# Or browse on GitHub
open https://github.com/wjllance/homebrew-standx-cli/blob/main/standx-cli.rb
```

### 2. Verify the Download URL

The formula should download from the official GitHub releases:

```ruby
# Expected URL pattern
url "https://github.com/wjllance/standx-cli/releases/download/v#{version}/standx-v#{version}-..."
```

### 3. Check SHA256 Hash

The formula includes SHA256 hashes for verification:

```ruby
sha256 "..."
```

## Alternative: Direct Download with Verification

If you prefer not to use the third-party tap, use the direct download method with checksum verification:

```bash
# Download from official GitHub releases
curl -L -o /tmp/standx.tar.gz https://github.com/wjllance/standx-cli/releases/download/v0.4.2/standx-v0.4.2-x86_64-unknown-linux-gnu.tar.gz

# Download checksums
curl -L -o /tmp/checksums.txt https://github.com/wjllance/standx-cli/releases/download/v0.4.2/checksums.txt

# Verify
cd /tmp && sha256sum -c checksums.txt --ignore-missing

# Install
sudo mv /tmp/standx /usr/local/bin/
sudo chmod +x /usr/local/bin/standx
```

## Reporting Issues

If you discover any security concerns with the Homebrew formula, please report:
- GitHub Issues: https://github.com/wjllance/standx-cli/issues
- Homebrew Tap Issues: https://github.com/wjllance/homebrew-standx-cli/issues
