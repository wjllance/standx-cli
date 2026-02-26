# Git 工作规范

## 分支管理规则

### ✅ 必须遵守

1. **禁止直接推送 main 分支**
   - 所有变更必须通过 PR 合并
   - 禁止 `git push origin main`

2. **创建功能分支**
   ```bash
   git checkout -b feature/description
   # 或
   git checkout -b fix/issue-number
   ```

3. **提交 PR 流程**
   ```bash
   # 1. 创建分支
   git checkout -b feature/new-feature
   
   # 2. 提交变更
   git add -A
   git commit -m "feat: description"
   
   # 3. 推送到远程
   git push -u origin feature/new-feature
   
   # 4. 创建 PR（使用 gh CLI）
   gh pr create --title "feat: description" --body "Details..."
   
   # 5. 等待 CI 通过后合并
   gh pr merge
   ```

### 分支命名规范

| 类型 | 前缀 | 示例 |
|------|------|------|
| 功能 | `feature/` | `feature/kline-time-format` |
| 修复 | `fix/` | `fix/issue-5.1-websocket-auth` |
| 文档 | `docs/` | `docs/update-testing-guide` |
| 样式 | `style/` | `style/cargo-fmt-fixes` |
| 重构 | `refactor/` | `refactor/websocket-client` |

### 提交信息规范

使用 Conventional Commits：

```
<type>: <description>

[optional body]
```

类型：
- `feat`: 新功能
- `fix`: 修复
- `docs`: 文档
- `style`: 格式（不影响代码逻辑）
- `refactor`: 重构
- `test`: 测试
- `chore`: 构建/工具

---

记录时间: 2026-02-26
