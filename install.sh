#!/bin/sh
# StandX CLI One-line Installer
# Supports macOS (Intel/Apple Silicon) and Linux (x86_64/ARM64)

set -e

REPO="wjllance/standx-cli"
BINARY_NAME="standx"
INSTALL_DIR="/usr/local/bin"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Detect target platform
get_target() {
    local os=$(uname -s)
    local arch=$(uname -m)

    case "$os" in
        Darwin)
            case "$arch" in
                arm64|aarch64)
                    echo "aarch64-apple-darwin"
                    ;;
                *)
                    echo "${RED}Error: Unsupported macOS architecture: $arch${NC}" >&2
                    exit 1
                    ;;
            esac
            ;;
        Linux)
            case "$arch" in
                aarch64|arm64)
                    echo "aarch64-unknown-linux-gnu"
                    ;;
                x86_64|amd64)
                    echo "x86_64-unknown-linux-gnu"
                    ;;
                *)
                    echo "${RED}Error: Unsupported Linux architecture: $arch${NC}" >&2
                    exit 1
                    ;;
            esac
            ;;
        *)
            echo "${RED}Error: Unsupported operating system: $os${NC}" >&2
            exit 1
            ;;
    esac
}

# Get latest version tag
get_latest_tag() {
    local api_url="https://api.github.com/repos/${REPO}/releases/latest"
    local tag=$(curl -sSL "$api_url" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')

    if [ -z "$tag" ]; then
        echo "${RED}Error: Unable to get latest version information${NC}" >&2
        exit 1
    fi

    echo "$tag"
}

# Main installation logic
main() {
    echo "${GREEN}=== StandX CLI Installer ===${NC}"
    echo ""

    # Detect platform
    local target=$(get_target)
    echo "Detected platform: ${YELLOW}$target${NC}"

    # Get latest version
    echo "Fetching latest version information..."
    local tag=$(get_latest_tag)
    echo "Latest version: ${YELLOW}$tag${NC}"

    # Construct download URL
    local tarball_name="${BINARY_NAME}-${tag}-${target}.tar.gz"
    local download_url="https://github.com/${REPO}/releases/download/${tag}/${tarball_name}"
    local checksums_url="https://github.com/${REPO}/releases/download/${tag}/checksums.txt"

    # Create temp directory
    local tmp_dir=$(mktemp -d)
    trap "rm -rf $tmp_dir" EXIT

    # Download tarball
    echo ""
    echo "Downloading ${tarball_name}..."
    if ! curl -sSL -o "${tmp_dir}/${tarball_name}" "$download_url"; then
        echo "${RED}Error: Download failed${NC}" >&2
        exit 1
    fi

    # Download checksums.txt
    echo "Downloading checksums.txt..."
    if ! curl -sSL -o "${tmp_dir}/checksums.txt" "$checksums_url"; then
        echo "${YELLOW}Warning: Unable to download checksums.txt, skipping verification${NC}"
    else
        # Verify SHA256
        echo "Verifying file integrity..."
        cd "$tmp_dir"
        if ! sha256sum -c checksums.txt --ignore-missing 2>/dev/null | grep -q "${tarball_name}: OK"; then
            echo "${RED}Error: SHA256 verification failed, file may be corrupted or tampered${NC}" >&2
            exit 1
        fi
        echo "${GREEN}✓ Verification passed${NC}"
        cd - >/dev/null
    fi

    # Extract
    echo ""
    echo "Extracting..."
    tar -xzf "${tmp_dir}/${tarball_name}" -C "$tmp_dir"

    # Check extracted binary
    local binary_path="${tmp_dir}/${BINARY_NAME}"
    if [ ! -f "$binary_path" ]; then
        echo "${RED}Error: Binary file ${BINARY_NAME} not found after extraction${NC}" >&2
        exit 1
    fi

    # Check install directory permissions
    if [ ! -d "$INSTALL_DIR" ]; then
        echo "${YELLOW}Install directory $INSTALL_DIR does not not exist, attempting to create...${NC}"
        if ! sudo mkdir -p "$INSTALL_DIR"; then
            echo "${RED}Error: Unable to create install directory${NC}" >&2
            exit 1
        fi
    fi

    # Install
    echo ""
    echo "Installing to ${INSTALL_DIR}/${BINARY_NAME}..."
    if [ -w "$INSTALL_DIR" ]; then
        mv "$binary_path" "${INSTALL_DIR}/${BINARY_NAME}"
        chmod +x "${INSTALL_DIR}/${BINARY_NAME}"
    else
        echo "${YELLOW}Administrator privileges required to install to $INSTALL_DIR${NC}"
        sudo mv "$binary_path" "${INSTALL_DIR}/${BINARY_NAME}"
        sudo chmod +x "${INSTALL_DIR}/${BINARY_NAME}"
    fi

    # Verify installation
    echo ""
    echo "Verifying installation..."
    if command -v "$BINARY_NAME" >/dev/null 2>&1; then
        local version=$($BINARY_NAME --version 2>/dev/null || echo "unknown")
        echo "${GREEN}✓ Installation successful!${NC}"
        echo ""
        echo "Version: ${YELLOW}$version${NC}"
        echo ""
        echo "Get started with:"
        echo "  ${YELLOW}standx --help${NC}          Show help"
        echo "  ${YELLOW}standx --version${NC}       Show version"
        echo "  ${YELLOW}standx auth login${NC}      Authenticate"
    else
        echo "${YELLOW}Warning: Installation complete, but $BINARY_NAME is not in PATH${NC}"
        echo "Please ensure $INSTALL_DIR is in your PATH environment variable"
    fi
}

# Run main function
main "$@"
