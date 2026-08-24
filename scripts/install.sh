#!/usr/bin/env bash
# OpenFlows Installer
# Usage: curl -fsSL https://get.openflows.dev | bash
#   or:  curl -fsSL https://raw.githubusercontent.com/The-AgenticFlow/AgentFlow/main/scripts/install.sh | bash
# Install latest stable:  curl -fsSL https://get.openflows.dev | bash
# Install edge (main):    curl -fsSL https://get.openflows.dev | bash -s -- --edge

set -euo pipefail

REPO="The-AgenticFlow/openflows"
INSTALL_DIR="${AGENTFLOW_INSTALL_DIR:-$HOME/.local/bin}"
BINARIES=("openflows" "openflows-doctor" "openflows-harness")
CHANNEL="stable"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

info()    { echo -e "${BLUE}  →${NC} $1"; }
success() { echo -e "${GREEN}  ✓${NC} $1"; }
warn()    { echo -e "${YELLOW}  ⚠${NC} $1"; }
fail()    { echo -e "${RED}  ✗${NC} $1" >&2; }

usage() {
    echo "Usage: $(basename "$0") [OPTIONS]"
    echo ""
    echo "Options:"
    echo "  --edge       Install the latest pre-release build from main"
    echo "  --stable     Install the latest stable release (default)"
    echo "  --dir DIR    Installation directory (default: ~/.local/bin)"
    echo "  -h, --help   Show this help message"
    echo ""
    echo "Environment variables:"
    echo "  AGENTFLOW_INSTALL_DIR  Installation directory (default: ~/.local/bin)"
    echo "  AGENTFLOW_CHANNEL      Release channel: stable or edge (default: stable)"
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --edge)   CHANNEL="edge"; shift ;;
        --stable) CHANNEL="stable"; shift ;;
        --dir)   INSTALL_DIR="$2"; shift 2 ;;
        -h|--help) usage; exit 0 ;;
        *) fail "Unknown option: $1"; usage; exit 1 ;;
    esac
done

if [[ -n "${AGENTFLOW_CHANNEL:-}" ]]; then
    CHANNEL="$AGENTFLOW_CHANNEL"
fi

detect_platform() {
    local os arch
    os=$(uname -s | tr '[:upper:]' '[:lower:]')
    arch=$(uname -m)
    case "$arch" in
        x86_64) arch="x86_64" ;;
        aarch64|arm64) arch="aarch64" ;;
        *) fail "Unsupported architecture: $arch"; exit 1 ;;
    esac
    case "$os" in
        darwin) os="apple-darwin" ;;
        linux)
            if ldd --version 2>&1 | grep -qi musl; then
                os="unknown-linux-musl"
            else
                os="unknown-linux-gnu"
            fi
            ;;
        *) fail "Unsupported OS: $os"; exit 1 ;;
    esac
    echo "${arch}-${os}"
}

has_cmd() { command -v "$1" &>/dev/null; }

ensure_rust() {
    if has_cmd rustc; then
        success "Rust $(rustc --version)"
        return
    fi
    info "Rust not found. Installing rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    if [ -f "$HOME/.cargo/env" ]; then
        source "$HOME/.cargo/env"
    fi
    success "Rust $(rustc --version)"
}

ensure_git() {
    if has_cmd git; then
        success "Git $(git --version)"
    else
        fail "Git is required. Please install git first."
        exit 1
    fi
}

ensure_node() {
    if has_cmd node; then
        local node_ver
        node_ver=$(node --version)
        local major
        major=$(echo "$node_ver" | cut -d. -f1 | tr -d 'v')
        if [ "$major" -ge 18 ]; then
            success "Node.js $node_ver"
            return
        fi
        warn "Node.js $node_ver is too old (need 18+). Installing via nvm..."
    else
        info "Node.js not found. Installing via nvm..."
    fi
    curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.1/install.sh | bash
    export NVM_DIR="$HOME/.nvm"
    [ -s "$NVM_DIR/nvm.sh" ] && . "$NVM_DIR/nvm.sh"
    nvm install 20
    success "Node.js $(node --version)"
}

ensure_ai_cli() {
    local has_claude=false
    local has_codex=false

    if has_cmd claude; then
        success "Claude Code CLI $(claude --version 2>/dev/null || echo 'installed')"
        has_claude=true
    fi

    if has_cmd codex; then
        success "Codex CLI $(codex --version 2>/dev/null || echo 'installed')"
        has_codex=true
    fi

    if [ "$has_claude" = false ] && [ "$has_codex" = false ]; then
        warn "No AI CLI backend found."
        echo ""
        echo "  OpenFlows requires either Claude Code (Anthropic) or Codex (OpenAI)."
        echo ""
        echo "  To install Claude Code:"
        echo "    npm install -g @anthropic-ai/claude-code"
        echo "    claude login"
        echo ""
        echo "  To install Codex:"
        echo "    npm install -g @openai/codex"
        echo "    codex login"
        echo ""
        echo "  You can also use npx without installing globally:"
        echo "    npx @anthropic-ai/claude-code"
        echo "    npx @openai/codex"
        echo ""
        info "Continuing without AI CLI - you'll need to install one before running OpenFlows"
    fi
}

# Copy orchestration files, preserving user-customized registry.json in OPENFLOWS_HOME.
install_orchestration() {
    local src_dir="$1"
    local oh_dir="${OPENFLOWS_HOME:-$HOME/.openflows}"
    local reg_path="orchestration/agent/registry.json"
    local backup=""

    if [ -f "${oh_dir}/${reg_path}" ]; then
        backup=$(mktemp)
        cp "${oh_dir}/${reg_path}" "$backup"
    fi

    cp -r "${src_dir}/orchestration" "${INSTALL_DIR}/"
    success "Installed orchestration config to ${INSTALL_DIR}/orchestration/"

    if [ -n "$backup" ] && [ -f "$backup" ]; then
        mkdir -p "${oh_dir}/orchestration/agent"
        cp "$backup" "${oh_dir}/${reg_path}"
        rm -f "$backup"
        info "Preserved existing registry.json in ${oh_dir} (run 'openflows-setup' to reconfigure)"
    fi
}

# Download a single release tarball into /tmp, trying the musl fallback for
# platforms without a native binary.
download_one() {
    local pkg="$1"
    local version="$2"
    local platform="$3"
    local out_var="$4"          # name of the var to set with the downloaded archive
    local asset_name="${pkg}-${version}-${platform}.tar.gz"
    local url="https://github.com/${REPO}/releases/download/${pkg}-${version}/${asset_name}"
    local downloaded=""

    if has_cmd curl; then
        if curl -fsSL "$url" -o "/tmp/${asset_name}" 2>/dev/null; then
            downloaded="$asset_name"
        fi
    elif has_cmd wget; then
        if wget -q "$url" -O "/tmp/${asset_name}" 2>/dev/null; then
            downloaded="$asset_name"
        fi
    else
        fail "Neither curl nor wget found"
        return 1
    fi

    if [ -z "$downloaded" ]; then
        local alt_platform=""
        case "$platform" in
            x86_64-unknown-linux-gnu)  alt_platform="x86_64-unknown-linux-musl" ;;
            aarch64-unknown-linux-gnu) alt_platform="aarch64-unknown-linux-musl" ;;
            *) ;;
        esac
        if [ -n "$alt_platform" ]; then
            local alt_asset="${pkg}-${version}-${alt_platform}.tar.gz"
            local alt_url="https://github.com/${REPO}/releases/download/${pkg}-${version}/${alt_asset}"
            warn "No ${pkg} binary for ${platform}, trying ${alt_platform}..."
            if has_cmd curl && curl -fsSL "$alt_url" -o "/tmp/${alt_asset}" 2>/dev/null; then
                downloaded="$alt_asset"
            elif has_cmd wget && wget -q "$alt_url" -O "/tmp/${alt_asset}" 2>/dev/null; then
                downloaded="$alt_asset"
            fi
        fi
    fi

    if [ -z "$downloaded" ]; then
        return 1
    fi

    printf -v "$out_var" "%s" "$downloaded"
    tar -xzf "/tmp/${downloaded}" -C /tmp/
    rm -f "/tmp/${downloaded}"
    return 0
}

download_binary() {
    local platform="$1"
    local main_version="$2"
    local harness_version="${3:-$main_version}"

    info "Downloading OpenFlows ${main_version} for ${platform}..."

    local main_archive=""
    local harness_archive=""
    if ! download_one "openflows" "$main_version" "$platform" main_archive; then
        fail "Failed to download openflows binary for ${platform}"
        info "Falling back to building from source..."
        return 1
    fi

    # The harness ships in its own release, possibly at its own version;
    # download it too, but don't fail the whole install if a particular
    # platform has no harness build.
    local harness_copied=false
    if download_one "openflows-harness" "$harness_version" "$platform" harness_archive; then
        harness_copied=true
    else
        warn "No openflows-harness binary for ${platform}; skipping harness"
    fi

    local extract_dir="/tmp/openflows-${main_version}-${platform}"
    for bin in "${BINARIES[@]}"; do
        if [ -f "${extract_dir}/${bin}" ]; then
            cp "${extract_dir}/${bin}" "${INSTALL_DIR}/"
            chmod +x "${INSTALL_DIR}/${bin}"
        fi
    done

    # The harness archive extracts to its own package-named directory, so copy
    # it separately rather than searching the main package's directory.
    if [ "$harness_copied" = true ]; then
        local harness_dir="/tmp/openflows-harness-${harness_version}-${platform}"
        if [ -f "${harness_dir}/openflows-harness" ]; then
            cp "${harness_dir}/openflows-harness" "${INSTALL_DIR}/"
            chmod +x "${INSTALL_DIR}/openflows-harness"
            success "Installed openflows-harness"
        fi
        rm -rf "${harness_dir}"
    fi

    if [ -d "${extract_dir}/orchestration" ]; then
        install_orchestration "${extract_dir}"
    fi

    rm -rf "${extract_dir}"

    success "Downloaded and extracted to ${INSTALL_DIR}/"
    return 0
}

build_from_source() {
    info "Building OpenFlows from source..."
    local repo_dir
    repo_dir=$(mktemp -d)
    trap "rm -rf '$repo_dir'" RETURN

    git clone --depth 1 "https://github.com/${REPO}.git" "$repo_dir"
    cd "$repo_dir"

    cargo build --release -p openflows -p openflows-harness

    for bin in "${BINARIES[@]}"; do
        if [ -f "target/release/${bin}" ]; then
            cp "target/release/${bin}" "${INSTALL_DIR}/"
            chmod +x "${INSTALL_DIR}/${bin}"
            success "Built and installed ${bin}"
        fi
    done

    if [ -d "orchestration" ]; then
        install_orchestration "."
    fi
}

# Resolve the version (without prefix) of the latest release for a package.
#
# `tag_prefix` must be the exact tag prefix for that package: `openflows-` for
# the main release and `openflows-harness-` for the harness. `openflows-harness-`
# tags share the `openflows-` prefix with the main package's tags, so `exclude_prefix`
# (e.g. `openflows-harness-` when resolving the main release) stops a newer harness
# tag from being mistaken for the main package. Each package is resolved
# independently so the two can be released at their own versions.
#
# `prerelease` selects the channel: "false" for stable, "true" for edge.
resolve_tag() {
    local tag_prefix="$1"
    local prerelease="$2"
    local exclude_prefix="${3:-}"
    local tag=""
    if has_cmd gh; then
        if [ -n "$exclude_prefix" ]; then
            tag=$(gh release list --repo "$REPO" --limit 100 --json tagName,isPrerelease \
                -q --arg pfx "$tag_prefix" --arg ex "$exclude_prefix" --arg pre "$prerelease" \
                '[.[] | select(.tagName | startswith($pfx)) | select((.tagName | startswith($ex)) | not)] | map(select((.isPrerelease | tostring) == $pre)) | .[0].tagName' 2>/dev/null || echo "")
        else
            tag=$(gh release list --repo "$REPO" --limit 100 --json tagName,isPrerelease \
                -q --arg pfx "$tag_prefix" --arg pre "$prerelease" \
                '[.[] | select(.tagName | startswith($pfx))] | map(select((.isPrerelease | tostring) == $pre)) | .[0].tagName' 2>/dev/null || echo "")
        fi
    fi
    if [ -z "$tag" ]; then
        if [ -n "$exclude_prefix" ]; then
            tag=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases?per_page=100" 2>/dev/null \
                | jq -r --arg pfx "$tag_prefix" --arg ex "$exclude_prefix" --arg pre "$prerelease" \
                '[.[] | select((.prerelease | tostring) == $pre) | select(.tag_name | startswith($pfx)) | select((.tag_name | startswith($ex)) | not)] | .[0].tag_name' 2>/dev/null || echo "")
        else
            tag=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases?per_page=100" 2>/dev/null \
                | jq -r --arg pfx "$tag_prefix" --arg pre "$prerelease" \
                '[.[] | select((.prerelease | tostring) == $pre) | select(.tag_name | startswith($pfx))] | .[0].tag_name' 2>/dev/null || echo "")
        fi
    fi
    echo "${tag#${tag_prefix}}"
}

ensure_path() {
    if ! echo "$PATH" | tr ':' '\n' | grep -qx "$INSTALL_DIR"; then
        warn "${INSTALL_DIR} is not in your PATH"
        local shell_rc=""
        case "$SHELL" in
            */bash)  shell_rc="$HOME/.bashrc" ;;
            */zsh)   shell_rc="$HOME/.zshrc" ;;
            */fish)  shell_rc="$HOME/.config/fish/config.fish" ;;
        esac
        if [ -n "$shell_rc" ]; then
            if [[ "$SHELL" == */fish ]]; then
                echo "fish_add_path $INSTALL_DIR" >> "$shell_rc"
            else
                echo "export PATH=\"\$PATH:$INSTALL_DIR\"" >> "$shell_rc"
            fi
            info "Added $INSTALL_DIR to PATH in $shell_rc"
            info "Run 'source $shell_rc' or restart your terminal"
        fi
    fi
}

run_setup() {
    echo ""
    echo "Would you like to run the setup wizard now? (Y/n)"
    read -r response
    if [[ "$response" =~ ^[Yy]$ ]] || [[ -z "$response" ]]; then
        if has_cmd openflows-setup; then
            openflows-setup
        else
            warn "openflows-setup not found in PATH. Run it manually after adding $INSTALL_DIR to PATH."
        fi
    fi
}

main() {
    echo ""
    if [ "$CHANNEL" = "edge" ]; then
        echo "╔══════════════════════════════════════════════╗"
        echo "║   OpenFlows Installer (EDGE channel)       ║"
        echo "║   ⚠ Pre-release build from main branch      ║"
        echo "╚══════════════════════════════════════════════╝"
    else
        echo "╔══════════════════════════════════════════════╗"
        echo "║        OpenFlows Installer                   ║"
        echo "║        Autonomous AI Development Team        ║"
        echo "╚══════════════════════════════════════════════╝"
    fi
    echo ""

    local platform
    platform=$(detect_platform)
    info "Platform: $platform"
    info "Install directory: $INSTALL_DIR"
    info "Channel: $CHANNEL"
    echo ""

    info "Checking prerequisites..."
    ensure_rust
    ensure_git
    ensure_node
    ensure_ai_cli
    echo ""

    mkdir -p "$INSTALL_DIR"
    local installed=false
    local tag=""

    if [ "$CHANNEL" = "edge" ]; then
        tag=$(resolve_tag "openflows-" "true" "openflows-harness-")
        local harness_tag
        harness_tag=$(resolve_tag "openflows-harness-" "true")
        if [ -n "$tag" ]; then
            info "Found edge release: $tag"
            if download_binary "$platform" "$tag" "$harness_tag"; then
                installed=true
            fi
        else
            warn "No edge release found. Falling back to building from latest main..."
        fi
    else
        tag=$(resolve_tag "openflows-" "false" "openflows-harness-")
        local harness_tag
        harness_tag=$(resolve_tag "openflows-harness-" "false")
        if [ -n "$tag" ]; then
            if download_binary "$platform" "$tag" "$harness_tag"; then
                installed=true
            fi
        fi
    fi

    if [ "$installed" = false ]; then
        build_from_source
    fi

    echo ""
    ensure_path

    echo ""
    echo "╔══════════════════════════════════════════════╗"
    echo "║        Installation Complete!                ║"
    echo "╚══════════════════════════════════════════════╝"
    echo ""
    if [ "$CHANNEL" = "edge" ]; then
        echo "  ⚠ Installed EDGE (pre-release) build: $tag"
        echo "    For stability, use: curl -fsSL https://get.openflows.dev | bash"
        echo ""
    fi
    echo "  Available commands:"
    echo "    openflows                  - Start orchestration"
    echo "    openflows --reset-orchestration - Reset config files to defaults"
    echo "    openflows-setup           - Guided setup wizard"
    echo "    openflows-dashboard       - Live monitoring TUI"
    echo "    openflows-doctor          - Diagnostic checks"
    echo "    openflows-harness         - Workspace automation CLI (for agent workers)"
    echo ""
    echo "  Next steps:"
    echo "    1. Run 'openflows-setup' to configure API keys"
    echo "    2. Run 'openflows' to start the autonomous team"
    echo ""
    echo "  Docs: https://openflows.dev"
    echo ""

    run_setup
}

main "$@"