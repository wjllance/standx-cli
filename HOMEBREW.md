# Homebrew Installation Guide

## For Users

### Install via Homebrew

```bash
# Add the tap
brew tap wjllance/standx-cli

# Install
brew install standx-cli

# Verify installation
standx --version
```

### Upgrade

```bash
brew update
brew upgrade standx-cli
```

### Uninstall

```bash
brew uninstall standx-cli
brew untap wjllance/standx-cli
```

## For Maintainers

### Setting Up Homebrew Tap

1. Create a new GitHub repository named `homebrew-standx-cli`
2. Add the formula file to the repository

```bash
# Clone the tap repository
git clone https://github.com/wjllance/homebrew-standx-cli.git
cd homebrew-standx-cli

# Copy the formula
cp ../standx-cli/homebrew/standx-cli.rb .

# Update SHA256
# Download the release tarball and calculate SHA256:
curl -sL https://github.com/wjllance/standx-cli/archive/refs/tags/v0.1.0.tar.gz | shasum -a 256

# Update the formula with the correct SHA256
# Commit and push
git add standx-cli.rb
git commit -m "Update standx-cli to v0.1.0"
git push origin main
```

### Updating the Formula

When releasing a new version:

1. Update the `url` in the formula to point to the new release
2. Update the `sha256` with the new checksum
3. Update the `version` if changed

```ruby
class StandxCli < Formula
  desc "CLI tool for StandX perpetual DEX"
  homepage "https://github.com/wjllance/standx-cli"
  url "https://github.com/wjllance/standx-cli/archive/refs/tags/v0.2.0.tar.gz"
  sha256 "NEW_SHA256_HERE"
  license "MIT"

  depends_on "rust" => :build

  def install
    system "cargo", "build", "--release", "--bin", "standx"
    bin.install "target/release/standx"
  end

  test do
    assert_match "standx #{version}", shell_output("#{bin}/standx --version")
    assert_match "A CLI tool for StandX perpetual DEX", shell_output("#{bin}/standx --help")
  end
end
```

### Testing the Formula Locally

```bash
# Test formula syntax
brew audit --new-formula ./standx-cli.rb

# Install from local formula
brew install --build-from-source ./standx-cli.rb

# Test the installation
brew test ./standx-cli.rb
```

## Troubleshooting

### Build Failures

If the build fails due to Rust version:
```bash
# Update Rust
rustup update

# Or install via Homebrew
brew install rust
```

### Permission Issues

If you encounter permission issues:
```bash
# Fix Homebrew permissions
sudo chown -R $(whoami) $(brew --prefix)/*
```

## Alternative: Direct Binary Download

For users who don't want to build from source, provide pre-built binaries:

```bash
# Download pre-built binary for macOS (Apple Silicon)
curl -L -o standx https://github.com/wjllance/standx-cli/releases/download/v0.1.0/standx-darwin-arm64
chmod +x standx
sudo mv standx /usr/local/bin/
```
