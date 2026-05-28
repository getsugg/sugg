#!/usr/bin/env bash
#
# Sugg Unix Installation Script (Linux & macOS)
#
# Usage:
#   curl -fsSL "https://raw.githubusercontent.com/axuj/sugg/main/scripts/install.sh" | bash

set -euo pipefail

# ==========================================
# Configuration (Modify for your repository)
# ==========================================
GITHUB_REPO="axuj/sugg"

# Colors for terminal output
CYAN='\033[0;36m'
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${CYAN}Starting Sugg installation...${NC}"

# 1. Detect OS and Architecture
OS="$(uname -s)"
ARCH="$(uname -m)"

if [ "$OS" = "Linux" ]; then
    if [ "$ARCH" = "x86_64" ] || [ "$ARCH" = "amd64" ]; then
        ASSET_NAME="sugg-x86_64-unknown-linux-gnu.tar.gz"
    else
        echo -e "${RED}Unsupported architecture: $ARCH on Linux${NC}"
        exit 1
    fi
elif [ "$OS" = "Darwin" ]; then
    if [ "$ARCH" = "x86_64" ] || [ "$ARCH" = "amd64" ]; then
        ASSET_NAME="sugg-x86_64-apple-darwin.tar.gz"
    elif [ "$ARCH" = "arm64" ] || [ "$ARCH" = "aarch64" ]; then
        ASSET_NAME="sugg-aarch64-apple-darwin.tar.gz"
    else
        echo -e "${RED}Unsupported architecture: $ARCH on macOS${NC}"
        exit 1
    fi
else
    echo -e "${RED}Unsupported OS: $OS${NC}"
    exit 1
fi

# Define Installation Directories
if [ -n "${SUGG_HOME:-}" ]; then
    INSTALL_DIR="$SUGG_HOME"
else
    INSTALL_DIR="$HOME/.sugg"
fi
BIN_DIR="$INSTALL_DIR/bin"

# 2. Fetch latest Release info
echo -e "Connecting to GitHub to fetch version info..."
RELEASE_API_URL="https://api.github.com/repos/$GITHUB_REPO/releases/latest"

if command -v curl >/dev/null 2>&1; then
    VERSION=$(curl -fsSL "$RELEASE_API_URL" | grep '"tag_name":' | head -n 1 | awk -F '"' '{print $4}')
elif command -v wget >/dev/null 2>&1; then
    VERSION=$(wget -qO- "$RELEASE_API_URL" | grep '"tag_name":' | head -n 1 | awk -F '"' '{print $4}')
else
    echo -e "${RED}Neither curl nor wget was found. Please install one to proceed.${NC}"
    exit 1
fi

if [ -z "$VERSION" ]; then
    echo -e "${RED}Failed to fetch version info. Please check your internet connection or repository name ($GITHUB_REPO).${NC}"
    exit 1
fi
echo -e "Found latest version: ${GREEN}$VERSION${NC}"

# 3. Download and Extract
DOWNLOAD_URL="https://github.com/$GITHUB_REPO/releases/download/$VERSION/$ASSET_NAME"
TEMP_DIR=$(mktemp -d)
trap 'rm -rf "$TEMP_DIR"' EXIT
TEMP_TAR="$TEMP_DIR/$ASSET_NAME"

echo -e "Downloading binaries from $DOWNLOAD_URL..."
if command -v curl >/dev/null 2>&1; then
    curl -fL "$DOWNLOAD_URL" -o "$TEMP_TAR"
else
    wget -qO "$TEMP_TAR" "$DOWNLOAD_URL"
fi

echo -e "Extracting and installing to $INSTALL_DIR..."
mkdir -p "$BIN_DIR"

tar -xzf "$TEMP_TAR" -C "$TEMP_DIR"

# Search recursively for executables in case of nested folders in the tarball
SUGG_BIN=$(find "$TEMP_DIR" -type f -name "sugg" | head -n 1)
SUGG_ENGINE_BIN=$(find "$TEMP_DIR" -type f -name "sugg-engine" | head -n 1)

if [ -n "$SUGG_BIN" ] && [ -n "$SUGG_ENGINE_BIN" ]; then
    mv -f "$SUGG_BIN" "$BIN_DIR/sugg"
    mv -f "$SUGG_ENGINE_BIN" "$INSTALL_DIR/sugg-engine"
    chmod +x "$BIN_DIR/sugg"
    chmod +x "$INSTALL_DIR/sugg-engine"
else
    echo -e "${RED}Missing sugg or sugg-engine in the downloaded archive!${NC}"
    exit 1
fi

# 4. Configure PATH environment variable
echo -e "Configuring environment variables..."
CURRENT_SHELL=$(basename "$SHELL")

if [ "$CURRENT_SHELL" = "fish" ]; then
    echo -e "${YELLOW}Please run the following to add sugg to your PATH and enable shell integration:${NC}"
    echo -e "  ${CYAN}sugg init fish | source${NC}"
    echo -e "${YELLOW}Then add 'sugg init fish | source' to your config.fish to persist.${NC}"
else
    if [ "$CURRENT_SHELL" = "zsh" ]; then
        DETECTED_PROFILE="$HOME/.zshrc"
    elif [ "$CURRENT_SHELL" = "bash" ]; then
        if [ -f "$HOME/.bashrc" ]; then
            DETECTED_PROFILE="$HOME/.bashrc"
        elif [ -f "$HOME/.bash_profile" ]; then
            DETECTED_PROFILE="$HOME/.bash_profile"
        else
            DETECTED_PROFILE="$HOME/.profile"
        fi
    else
        DETECTED_PROFILE="$HOME/.profile"
    fi

    if [ ! -f "$DETECTED_PROFILE" ]; then
        touch "$DETECTED_PROFILE"
    fi

    if grep -q "$BIN_DIR" "$DETECTED_PROFILE"; then
        echo -e "${GREEN}PATH config already exists in $DETECTED_PROFILE, skipping.${NC}"
    else
        echo -e "\n# added by sugg install\nexport PATH=\"$BIN_DIR:\$PATH\"" >> "$DETECTED_PROFILE"
        echo -e "${GREEN}Added $BIN_DIR to $DETECTED_PROFILE.${NC}"
        echo -e "${YELLOW}Note: Please restart your terminal or run 'source $DETECTED_PROFILE' for PATH changes to take effect.${NC}"
        echo -e "${YELLOW}Then run 'sugg init $CURRENT_SHELL' to enable shell integration.${NC}"
    fi
fi

echo ""
echo -e "${GREEN}Sugg ($VERSION) installed successfully!${NC}"
echo "   sugg          -> $BIN_DIR/sugg"
echo "   sugg-engine   -> $INSTALL_DIR/sugg-engine"
echo ""
echo -e "${CYAN}Please restart your terminal and type 'sugg' to get started.${NC}"
