#!/bin/sh
# StandX CLI 一键安装脚本
# 支持 macOS (Intel/Apple Silicon) 和 Linux (x86_64/ARM64)

set -e

REPO="wjllance/standx-cli"
BINARY_NAME="standx"
INSTALL_DIR="/usr/local/bin"

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# 检测目标平台
get_target() {
    local os=$(uname -s)
    local arch=$(uname -m)

    case "$os" in
        Darwin)
            case "$arch" in
                arm64|aarch64)
                    echo "aarch64-apple-darwin"
                    ;;
                x86_64|amd64)
                    echo "x86_64-apple-darwin"
                    ;;
                *)
                    echo "${RED}错误: 不支持的 macOS 架构: $arch${NC}" >&2
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
                    echo "${RED}错误: 不支持的 Linux 架构: $arch${NC}" >&2
                    exit 1
                    ;;
            esac
            ;;
        *)
            echo "${RED}错误: 不支持的操作系统: $os${NC}" >&2
            exit 1
            ;;
    esac
}

# 获取最新版本标签
get_latest_tag() {
    local api_url="https://api.github.com/repos/${REPO}/releases/latest"
    local tag=$(curl -sSL "$api_url" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')

    if [ -z "$tag" ]; then
        echo "${RED}错误: 无法获取最新版本信息${NC}" >&2
        exit 1
    fi

    echo "$tag"
}

# 主安装逻辑
main() {
    echo "${GREEN}=== StandX CLI 安装脚本 ===${NC}"
    echo ""

    # 检测平台
    local target=$(get_target)
    echo "检测到平台: ${YELLOW}$target${NC}"

    # 获取最新版本
    echo "正在获取最新版本信息..."
    local tag=$(get_latest_tag)
    echo "最新版本: ${YELLOW}$tag${NC}"

    # 构造下载 URL
    local tarball_name="${BINARY_NAME}-${tag}-${target}.tar.gz"
    local download_url="https://github.com/${REPO}/releases/download/${tag}/${tarball_name}"
    local checksums_url="https://github.com/${REPO}/releases/download/${tag}/checksums.txt"

    # 创建临时目录
    local tmp_dir=$(mktemp -d)
    trap "rm -rf $tmp_dir" EXIT

    # 下载 tarball
    echo ""
    echo "正在下载 ${tarball_name}..."
    if ! curl -sSL -o "${tmp_dir}/${tarball_name}" "$download_url"; then
        echo "${RED}错误: 下载失败${NC}" >&2
        exit 1
    fi

    # 下载 checksums.txt
    echo "正在下载 checksums.txt..."
    if ! curl -sSL -o "${tmp_dir}/checksums.txt" "$checksums_url"; then
        echo "${YELLOW}警告: 无法下载 checksums.txt，跳过校验${NC}"
    else
        # 校验 SHA256
        echo "正在校验文件完整性..."
        cd "$tmp_dir"
        if ! sha256sum -c checksums.txt --ignore-missing 2>/dev/null | grep -q "${tarball_name}: OK"; then
            echo "${RED}错误: SHA256 校验失败，文件可能损坏或被篡改${NC}" >&2
            exit 1
        fi
        echo "${GREEN}✓ 校验通过${NC}"
        cd - >/dev/null
    fi

    # 解压
    echo ""
    echo "正在解压..."
    tar -xzf "${tmp_dir}/${tarball_name}" -C "$tmp_dir"

    # 检查解压后的二进制文件
    local binary_path="${tmp_dir}/${BINARY_NAME}"
    if [ ! -f "$binary_path" ]; then
        echo "${RED}错误: 解压后未找到 ${BINARY_NAME} 二进制文件${NC}" >&2
        exit 1
    fi

    # 检查安装目录权限
    if [ ! -d "$INSTALL_DIR" ]; then
        echo "${YELLOW}安装目录 $INSTALL_DIR 不存在，尝试创建...${NC}"
        if ! sudo mkdir -p "$INSTALL_DIR"; then
            echo "${RED}错误: 无法创建安装目录${NC}" >&2
            exit 1
        fi
    fi

    # 安装
    echo ""
    echo "正在安装到 ${INSTALL_DIR}/${BINARY_NAME}..."
    if [ -w "$INSTALL_DIR" ]; then
        mv "$binary_path" "${INSTALL_DIR}/${BINARY_NAME}"
        chmod +x "${INSTALL_DIR}/${BINARY_NAME}"
    else
        echo "${YELLOW}需要管理员权限来安装到 $INSTALL_DIR${NC}"
        sudo mv "$binary_path" "${INSTALL_DIR}/${BINARY_NAME}"
        sudo chmod +x "${INSTALL_DIR}/${BINARY_NAME}"
    fi

    # 验证安装
    echo ""
    echo "验证安装..."
    if command -v "$BINARY_NAME" >/dev/null 2>&1; then
        local version=$($BINARY_NAME --version 2>/dev/null || echo "unknown")
        echo "${GREEN}✓ 安装成功!${NC}"
        echo ""
        echo "版本: ${YELLOW}$version${NC}"
        echo ""
        echo "使用以下命令开始使用:"
        echo "  ${YELLOW}standx --help${NC}          查看帮助"
        echo "  ${YELLOW}standx --version${NC}       查看版本"
        echo "  ${YELLOW}standx auth login${NC}      登录认证"
    else
        echo "${YELLOW}警告: 安装完成，但 $BINARY_NAME 不在 PATH 中${NC}"
        echo "请确保 $INSTALL_DIR 在你的 PATH 环境变量中"
    fi
}

# 运行主函数
main "$@"
