#!/bin/bash
# GitHub Actions 状态监控脚本
# 使用 curl + grep 获取基本信息

REPO="wjllance/standx-cli"

echo "🔍 检查 GitHub Actions 状态..."
echo "=============================="

# 方法1: 使用 GitHub API 获取最新运行（无需认证，public repo）
echo -e "\n📊 方法1: GitHub API"
API_URL="https://api.github.com/repos/${REPO}/actions/runs?per_page=1"
RESPONSE=$(curl -s "${API_URL}")

# 解析结果
RUN_COUNT=$(echo "$RESPONSE" | grep -o '"total_count": [0-9]*' | cut -d' ' -f2)
if [ -n "$RUN_COUNT" ]; then
    echo "总运行次数: $RUN_COUNT"
    
    # 获取最新运行状态
    CONCLUSION=$(echo "$RESPONSE" | grep -o '"conclusion": "[^"]*"' | head -1 | cut -d'"' -f4)
    STATUS=$(echo "$RESPONSE" | grep -o '"status": "[^"]*"' | head -1 | cut -d'"' -f4)
    RUN_ID=$(echo "$RESPONSE" | grep -o '"id": [0-9]*' | head -1 | cut -d' ' -f2)
    HTML_URL=$(echo "$RESPONSE" | grep -o '"html_url": "[^"]*"' | head -1 | cut -d'"' -f4)
    
    echo "最新运行ID: $RUN_ID"
    echo "状态: $STATUS"
    echo "结果: $CONCLUSION"
    echo "链接: $HTML_URL"
    
    # 显示 emoji 状态
    if [ "$CONCLUSION" = "success" ]; then
        echo -e "\n✅ 最新构建: 成功"
    elif [ "$CONCLUSION" = "failure" ]; then
        echo -e "\n❌ 最新构建: 失败"
    else
        echo -e "\n⏳ 最新构建: $CONCLUSION"
    fi
else
    echo "无法获取数据"
fi

# 方法2: 获取 Status Badge
echo -e "\n📛 方法2: Status Badge"
BADGE_URL="https://github.com/${REPO}/workflows/CI/badge.svg"
BADGE=$(curl -sL "${BADGE_URL}")

if echo "$BADGE" | grep -q "passing"; then
    echo "Badge 状态: ✅ passing"
elif echo "$BADGE" | grep -q "failing"; then
    echo "Badge 状态: ❌ failing"
else
    echo "Badge 状态: 未知"
fi

# 方法3: 尝试获取页面内容（如果可用）
echo -e "\n🌐 方法3: 页面内容分析"
PAGE=$(curl -sL "https://github.com/${REPO}/actions" -H "User-Agent: Mozilla/5.0" 2>/dev/null | head -500)

if echo "$PAGE" | grep -q "success"; then
    echo "页面包含: success"
fi
if echo "$PAGE" | grep -q "failure"; then
    echo "页面包含: failure"
fi
if echo "$PAGE" | grep -q "completed"; then
    echo "页面包含: completed"
fi

echo -e "\n=============================="
echo "监控完成"
