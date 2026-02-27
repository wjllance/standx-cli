# ClawHub 发布目录

此目录专门用于发布到 ClawHub。

## 文件结构

```
clawhub-publish/
├── SKILL.md              # Skill 文档（必需）
├── skill.json            # Skill 元数据（必需）
├── README.md             # 本文件
└── references/           # 参考文档
    ├── api-docs.md
    ├── authentication.md
    ├── examples.md
    ├── homebrew.md
    └── troubleshooting.md
```

## 版本号说明

### 两个版本号的区别

| 版本号类型 | 说明 | 使用场景 | 示例 |
|-----------|------|---------|------|
| **CLI 版本号** | standx 命令行工具的实际版本 | GitHub Release、二进制文件版本 | `v0.4.2` |
| **Skill 版本号** | OpenClaw Skill 的元数据版本 | ClawHub 发布、skill 配置 | `0.4.6` |

### 版本号关系

```
GitHub Release (CLI)          ClawHub (Skill)
    v0.4.2         ←───→        0.4.6
    (二进制)                     (配置)
```

**关键区别：**
- **CLI 版本** = 实际可执行程序的版本，用户运行 `standx --version` 看到的
- **Skill 版本** = OpenClaw 集成配置的版本，包含安装方式、凭证声明等

### 为什么需要分开？

1. **CLI 不更新，但 Skill 配置需要更新**
   - 例：改进了 OpenClaw 的 credential 声明
   - CLI 还是 v0.4.2，但 Skill 从 0.4.5 → 0.4.6

2. **CLI 更新了，但 Skill 配置不变**
   - 例：修复了 CLI 的 bug，发布 v0.4.3
   - Skill 安装方式没变，可以保持 0.4.6

3. **独立演进**
   - CLI 版本由功能开发决定
   - Skill 版本由集成需求决定

### 版本更新场景

| 场景 | CLI 版本 | Skill 版本 | 操作 |
|------|---------|-----------|------|
| CLI 新增功能 | 更新 | 可选 | 发布 GitHub Release |
| CLI 修复 bug | 更新 | 可选 | 发布 GitHub Release |
| 改进 Skill 安装方式 | 不变 | 更新 | 发布 ClawHub |
| 添加 credential 声明 | 不变 | 更新 | 发布 ClawHub |
| 更新文档 | 不变 | 可选 | 提交 PR |

### 当前版本状态

- **CLI 最新版本**: v0.4.2 (GitHub Release)
- **Skill 最新版本**: 0.4.6 (ClawHub)

## 发布命令

```bash
cd clawhub-publish

# 发布新版本（Skill 版本号递增）
clawhub publish . --slug standx-cli --version 0.4.7 --changelog "更新说明"
```

## 版本更新流程

### 当 CLI 发布新版本时

1. 创建 GitHub Release（如 v0.4.3）
2. 可选：更新 Skill 中的版本引用
3. 如果 Skill 配置有变化，发布新版本到 ClawHub

### 当 Skill 配置更新时

1. 更新 `SKILL.md` 中的版本号（递增，如 0.4.6 → 0.4.7）
2. 更新 `skill.json` 中的配置
3. 提交代码到 GitHub（通过 PR）
4. 运行发布命令到 ClawHub

## 注意事项

- 此目录只包含发布所需的文件
- 不包含源代码、测试文件等
- 保持目录结构简洁
- **不要直接提交到 main 分支**，必须通过 PR 合并
