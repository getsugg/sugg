#!/usr/bin/env bash
#
# Sugg Unix Installation Script (Linux & macOS)
#
# Usage:
#   curl -fsSL "https://raw.githubusercontent.com/getsugg/sugg/main/scripts/install.sh" | bash

set -euo pipefail

# ==========================================
# Configuration (Modify for your repository)
# ==========================================
GITHUB_REPO="getsugg/sugg"

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
        ASSET_NAME="sugg-x86_64-unknown-linux-musl.tar.gz"
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

# 2. Download latest
DOWNLOAD_URL="https://github.com/$GITHUB_REPO/releases/latest/download/$ASSET_NAME"
TEMP_DIR=$(mktemp -d)
trap 'rm -rf "$TEMP_DIR"' EXIT
TEMP_TAR="$TEMP_DIR/$ASSET_NAME"

echo -e "Downloading binaries from $DOWNLOAD_URL..."
if command -v curl >/dev/null 2>&1; then
    curl -#fL "$DOWNLOAD_URL" -o "$TEMP_TAR"
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

# 3. PATH 提示
echo -e "${YELLOW}Please add the following to your shell profile:${NC}"
echo -e "  ${CYAN}export PATH=\"$BIN_DIR:\$PATH\"${NC}"
echo -e "${YELLOW}Then run 'sugg init <shell>' to enable shell integration.${NC}"

echo ""
echo -e "${GREEN}Sugg installed successfully!${NC}"
echo "   sugg          -> $BIN_DIR/sugg"
echo "   sugg-engine   -> $INSTALL_DIR/sugg-engine"
echo ""
echo -e "${CYAN}Please restart your terminal and type 'sugg' to get started.${NC}"
