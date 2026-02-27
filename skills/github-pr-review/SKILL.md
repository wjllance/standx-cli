---
name: github-pr-review
metadata:
  openclaw:
    emoji: 👁️
    requires:
      bins:
        - gh
    primaryCredential:
      kind: env
      env: GITHUB_TOKEN
      description: GitHub personal access token for PR review
    install:
      - id: brew
        kind: brew
        formula: gh
        bins:
          - gh
        label: Install GitHub CLI
---

# GitHub PR Review Skill

Automated PR review workflow for GitHub repositories.

## Use Cases

- Monitor repository for new PRs requiring review
- Automated code review with AI assistance
- Track review history and avoid duplicate reviews

## Workflow Steps

### 1. List Open PRs

```bash
gh pr list --state open --limit 10 --repo owner/repo
```

### 2. Filter PRs to Review

Skip PRs that are:
- Already reviewed by you
- Closed/merged
- Draft (optional)

### 3. Review Each PR

```bash
# View PR details
gh pr view <number> --repo owner/repo

# View diff
gh pr diff <number> --repo owner/repo

# Submit review
gh pr review <number> --comment --body "Review comment"
```

### 4. Record Reviewed PRs

Maintain a log file to avoid duplicate reviews:
```
pr-review-log.md
```

## Best Practices

### Reporting Rules

- **Send message**: Only when new PRs are reviewed
- **NO_REPLY**: If no new activity

### Review Criteria

1. Code quality and style
2. Security concerns
3. Documentation completeness
4. Test coverage

### Review Template

```markdown
## PR Review Summary

**Status**: [Approved/Request Changes/Comment]

### Summary
Brief description of changes

### Code Quality
- ✅/❌ Code style consistent
- ✅/❌ No obvious bugs
- ✅/❌ Tests included

### Suggestions
1. ...
2. ...

---
Reviewed by: Kimi Claw AI Assistant
```

## Cron Configuration

```json
{
  "name": "repo-pr-monitor",
  "schedule": {
    "everyMs": 600000
  },
  "message": "Check for new PRs..."
}
```

## References

- [GitHub CLI docs](https://cli.github.com/manual/)
- [PR Review best practices](https://docs.github.com/en/pull-requests/collaborating-with-pull-requests/reviewing-changes-in-pull-requests/about-pull-request-reviews)
