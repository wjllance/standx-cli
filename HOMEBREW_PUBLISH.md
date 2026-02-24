# Homebrew 发布指南

## 概述

将 standx-cli 发布到 Homebrew 有两种方式：
1. **自定义 Tap** (推荐) - 快速，完全控制
2. **Homebrew Core** - 需要审核，更正式

## 方式一：自定义 Tap (推荐)

### 步骤 1: 创建 Homebrew Tap 仓库

```bash
# 创建新的 GitHub 仓库: wjllance/homebrew-standx-cli
# 本地初始化
git clone https://github.com/wjllance/homebrew-standx-cli.git
cd homebrew-standx-cli
```

### 步骤 2: 创建 Formula

创建 `Formula/standx-cli.rb`:

```ruby
class StandxCli < Formula
  desc "CLI tool for StandX perpetual DEX"
  homepage "https://github.com/wjllance/standx-cli"
  url "https://github.com/wjllance/standx-cli/archive/refs/tags/v0.1.0.tar.gz"
  sha256 "PLACEHOLDER_SHA256"
  license "MIT"

  depends_on "rust" => :build

  def install
    system "cargo", "build", "--release", "--bin", "standx"
    bin.install "target/release/standx"
  end

  test do
    assert_match "standx #{version}", shell_output("#{bin}/standx --version")
  end
end
```

### 步骤 3: 计算 SHA256

```bash
# 下载 release tarball
curl -L -o standx-cli-0.1.0.tar.gz \
  https://github.com/wjllance/standx-cli/archive/refs/tags/v0.1.0.tar.gz

# 计算 SHA256
shasum -a 256 standx-cli-0.1.0.tar.gz
# 输出: abc123...  standx-cli-0.1.0.tar.gz
```

### 步骤 4: 提交并推送

```bash
git add Formula/standx-cli.rb
git commit -m "Add standx-cli formula v0.1.0"
git push origin main
```

### 步骤 5: 用户安装

用户现在可以：

```bash
# 添加 tap
brew tap wjllance/standx-cli

# 安装
brew install standx-cli

# 升级
brew upgrade standx-cli
```

## 方式二：Homebrew Core (正式)

### 要求

- 软件必须开源并有稳定版本
- 必须通过 `brew audit --new-formula` 检查
- 必须有合理的知名度（stars/forks）

### 步骤

```bash
# 1. Fork homebrew/core
git clone https://github.com/Homebrew/homebrew-core.git
cd homebrew-core

# 2. 创建新分支
git checkout -b add-standx-cli

# 3. 创建 formula
brew create https://github.com/wjllance/standx-cli/archive/refs/tags/v0.1.0.tar.gz

# 4. 编辑生成的 formula
# - 添加 description
# - 添加 homepage
# - 确认 license
# - 添加 depends_on

# 5. 测试安装
HOMEBREW_NO_INSTALL_FROM_API=1 brew install --build-from-source standx-cli

# 6. 运行测试
brew test standx-cli

# 7. 审计检查
brew audit --new-formula standx-cli

# 8. 提交并创建 PR
git add Formula/s/standx-cli.rb
git commit -m "standx-cli 0.1.0 (new formula)"
git push origin add-standx-cli
```

## 自动发布流程 (GitHub Actions)

### 1. 更新 CI 配置

`.github/workflows/release.yml`:

```yaml
name: Release

on:
  release:
    types: [published]

jobs:
  build-macos:
    runs-on: macos-latest
    steps:
    - uses: actions/checkout@v4
    
    - name: Install Rust
      uses: dtolnay/rust-toolchain@stable
    
    - name: Build
      run: cargo build --release
    
    - name: Upload binary
      uses: actions/upload-artifact@v4
      with:
        name: standx-macos
        path: target/release/standx

  update-homebrew:
    needs: build-macos
    runs-on: ubuntu-latest
    steps:
    - name: Checkout tap repo
      uses: actions/checkout@v4
      with:
        repository: wjllance/homebrew-standx-cli
        token: ${{ secrets.HOMEBREW_TAP_TOKEN }}
    
    - name: Download binary
      uses: actions/download-artifact@v4
      with:
        name: standx-macos
    
    - name: Calculate SHA256
      run: |
        chmod +x standx
        tar -czf standx.tar.gz standx
        echo "SHA256=$(sha256sum standx.tar.gz | cut -d' ' -f1)" >> $GITHUB_ENV
    
    - name: Update formula
      run: |
        sed -i "s/sha256 \".*\"/sha256 \"$SHA256\"/" Formula/standx-cli.rb
        sed -i "s/url \".*\"/url \"https:\/\/github.com\/wjllance\/standx-cli\/releases\/download\/${{ github.event.release.tag_name }}\/standx-macos.tar.gz\"/" Formula/standx-cli.rb
    
    - name: Commit and push
      run: |
        git config user.name "GitHub Actions"
        git config user.email "actions@github.com"
        git add Formula/standx-cli.rb
        git commit -m "Update standx-cli to ${{ github.event.release.tag_name }}"
        git push
```

### 2. 添加 GitHub Secret

在 standx-cli 仓库设置中添加：`HOMEBREW_TAP_TOKEN`
- 生成 Personal Access Token (classic)
- 权限：`repo` 和 `workflow`

## 版本更新流程

### 手动更新

```bash
# 1. 更新 formula 中的版本和 SHA256
vim Formula/standx-cli.rb

# 2. 测试
brew reinstall --build-from-source ./Formula/standx-cli.rb

# 3. 提交
git add Formula/standx-cli.rb
git commit -m "standx-cli 0.2.0"
git push
```

### 自动更新

使用 `brew bump-formula-pr`:

```bash
brew bump-formula-pr --url https://github.com/wjllance/standx-cli/archive/refs/tags/v0.2.0.tar.gz standx-cli
```

## 故障排除

### 1. SHA256 不匹配

```bash
# 重新计算
curl -L <url> | shasum -a 256
```

### 2. 依赖问题

```ruby
# 添加依赖
depends_on "openssl"
depends_on "pkg-config" => :build
```

### 3. 测试失败

```ruby
test do
  # 简单测试
  assert_match "usage", shell_output("#{bin}/standx --help 2>&1")
end
```

## 参考文档

- [Homebrew Formula Cookbook](https://docs.brew.sh/Formula-Cookbook)
- [Homebrew Tap 文档](https://docs.brew.sh/Taps)
- [brew bump-formula-pr](https://docs.brew.sh/Manpage#bump-formula-pr-options-formula)

## 快速检查清单

- [ ] 创建 `wjllance/homebrew-standx-cli` 仓库
- [ ] 添加 `Formula/standx-cli.rb`
- [ ] 计算正确的 SHA256
- [ ] 测试 `brew install` 本地
- [ ] 测试 `brew test` 通过
- [ ] 推送 formula 到 tap
- [ ] 验证 `brew tap wjllance/standx-cli` 工作
- [ ] 可选：设置自动更新 GitHub Actions
