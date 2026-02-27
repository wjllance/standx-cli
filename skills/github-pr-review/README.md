# GitHub PR Review Workflow

This directory contains a reusable skill for automated GitHub PR review.

## Created From

This workflow was extracted from the `standx-cli-pr-monitor` cron job, which monitors the standx-cli repository for new PRs requiring review.

## Key Learnings

### 1. PR Filtering

**Always filter by state:**
```bash
gh pr list --state open  # Only open PRs
```

**Skip conditions:**
- Closed/merged PRs
- Already reviewed by you
- Draft PRs (optional)

### 2. Avoid Duplicate Reviews

Check existing comments:
```bash
gh pr view <number> --comments | grep "your-username"
```

Maintain a review log file.

### 3. Smart Reporting

**Don't spam:**
- Send message only when there's new activity
- Use `NO_REPLY` when nothing to report

### 4. Review Template

Standardize review comments:
- Summary
- Code quality check
- Suggestions
- Signature

## Usage

### As a Cron Job

```bash
openclaw cron add --name "my-repo-review" \
  --schedule "every 10m" \
  --message "Check PRs for owner/repo"
```

### Manual Review

```bash
gh pr list --state open
gh pr diff <number>
gh pr review <number> --comment
```

## Files

- `SKILL.md` - Skill definition and workflow
- `README.md` - This file

## Future Improvements

- [ ] Auto-detect programming language
- [ ] Integration with CI status
- [ ] Review assignment based on file paths
