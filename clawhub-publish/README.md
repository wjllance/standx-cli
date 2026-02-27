# ClawHub 发布目录

此目录专门用于发布到 ClawHub。

## 文件结构

```
clawhub-publish/
├── SKILL.md              # Skill 文档（必需）
├── skill.json            # Skill 元数据（必需）
└── references/           # 参考文档
    ├── api-docs.md
    ├── authentication.md
    ├── examples.md
    └── troubleshooting.md
```

## 发布命令

```bash
cd clawhub-publish
clawhub publish . --slug standx-cli --version 0.4.4 --changelog "发布说明"
```

## 版本更新流程

1. 更新 `SKILL.md` 中的版本号
2. 更新 `skill.json` 中的下载 URL
3. 提交代码到 GitHub
4. 创建新的 GitHub Release
5. 运行发布命令

## 注意事项

- 此目录只包含发布所需的文件
- 不包含源代码、测试文件等
- 保持目录结构简洁
