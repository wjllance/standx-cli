# GitHub Actions Secrets 配置指南

## 需要的 Secrets

### 1. HOMEBREW_TAP_TOKEN

用于自动更新 Homebrew Tap 仓库。

#### 创建步骤：

1. **生成 Personal Access Token (Classic)**
   - 访问: https://github.com/settings/tokens
   - 点击 "Generate new token (classic)"
   - 选择以下权限:
     - `repo` - 访问仓库
     - `workflow` - 访问 workflow

2. **添加到仓库 Secrets**
   - 访问: https://github.com/wjllance/standx-cli/settings/secrets/actions
   - 点击 "New repository secret"
   - Name: `HOMEBREW_TAP_TOKEN`
   - Value: 粘贴刚才生成的 token
   - 点击 "Add secret"

## 自动发布流程

当你创建一个新的 Release 时，CI 会自动：

1. **构建二进制文件**
   - macOS 版本
   - Linux 版本

2. **上传到 GitHub Release**
   - `standx-macos-amd64.tar.gz`
   - `standx-linux-amd64.tar.gz`

3. **更新 Homebrew Formula**
   - 下载源码 tarball
   - 计算 SHA256
   - 更新 `wjllance/homebrew-standx-cli` 仓库

## 手动触发测试

如果你想测试自动更新功能，可以在 GitHub 上：

1. 进入 Actions 页面
2. 选择 "CI" workflow
3. 点击 "Run workflow"
4. 选择分支，添加输入参数

## 故障排除

### 如果 Homebrew 更新失败

检查以下事项：

1. **Token 权限**
   ```bash
   # 确保 token 有权限访问 homebrew-standx-cli 仓库
   ```

2. **仓库权限**
   - 确保 `wjllance/homebrew-standx-cli` 是公开的
   - 或者 token 有权限访问私有仓库

3. **查看日志**
   - 在 GitHub Actions 页面查看 `update-homebrew` job 的日志

### 手动更新 Formula

如果自动更新失败，可以手动更新：

```bash
# 1. 下载源码
curl -L -o standx-cli.tar.gz https://github.com/wjllance/standx-cli/archive/refs/tags/v0.1.1.tar.gz

# 2. 计算 SHA256
shasum -a 256 standx-cli.tar.gz

# 3. 更新 Formula
vim Formula/standx-cli.rb
# 更新 url 和 sha256

# 4. 提交
git add Formula/standx-cli.rb
git commit -m "Update standx-cli to v0.1.1"
git push
```
