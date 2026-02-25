#!/usr/bin/env python3
"""
GitHub Actions 日志抓取脚本 - 使用 Playwright
"""

import json
import sys
from playwright.sync_api import sync_playwright

REPO_OWNER = "wjllance"
REPO_NAME = "standx-cli"

def scrape_latest_action():
    """抓取最新的 GitHub Actions 运行结果"""
    
    with sync_playwright() as p:
        # 启动 headless 浏览器
        browser = p.chromium.launch(headless=True)
        context = browser.new_context(
            user_agent='Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.0'
        )
        page = context.new_page()
        
        try:
            # 访问 Actions 页面
            url = f"https://github.com/{REPO_OWNER}/{REPO_NAME}/actions"
            print(f"🌐 访问: {url}")
            page.goto(url, wait_until="networkidle")
            
            # 等待页面加载
            page.wait_for_timeout(3000)
            
            # 截图保存
            page.screenshot(path="/tmp/github_actions_overview.png")
            print("📸 已保存截图: /tmp/github_actions_overview.png")
            
            # 获取最新的工作流运行
            result = {
                "repo": f"{REPO_OWNER}/{REPO_NAME}",
                "timestamp": page.evaluate("() => new Date().toISOString()"),
                "runs": []
            }
            
            # 查找工作流运行列表
            # GitHub 的页面结构可能变化，这里使用多种选择器尝试
            selectors = [
                "[data-testid='workflow-run']",
                ".workflow-run",
                "[data-testid='run-item']",
                ".ActionList-item",
                "article[data-testid]"
            ]
            
            runs = []
            for selector in selectors:
                try:
                    elements = page.locator(selector).all()
                    if elements:
                        print(f"✅ 找到 {len(elements)} 个运行记录 (使用选择器: {selector})")
                        runs = elements[:5]  # 只取前5个
                        break
                except:
                    continue
            
            if not runs:
                print("⚠️ 未找到工作流运行记录，尝试备用方案...")
                # 备用：直接获取页面文本
                page_text = page.content()
                if "success" in page_text.lower():
                    result["detected_status"] = "success"
                elif "failure" in page_text.lower() or "failed" in page_text.lower():
                    result["detected_status"] = "failure"
                
                # 保存 HTML 用于分析
                with open("/tmp/github_actions_page.html", "w") as f:
                    f.write(page_text)
                print("📝 已保存页面 HTML: /tmp/github_actions_page_page.html")
            
            # 解析每个运行记录
            for i, run in enumerate(runs):
                try:
                    run_data = {"index": i}
                    
                    # 尝试获取状态
                    try:
                        # 查找状态图标
                        status_selectors = [
                            "[data-testid='run-status']",
                            ".status-icon",
                            "svg.octicon-check",
                            "svg.octicon-x",
                            ".octicon-check",
                            ".octicon-x",
                            "[aria-label*='success' i]",
                            "[aria-label*='fail' i]"
                        ]
                        
                        for sel in status_selectors:
                            try:
                                icon = run.locator(sel).first
                                if icon.count() > 0:
                                    aria = icon.get_attribute("aria-label") or ""
                                    if "success" in aria.lower() or "check" in sel:
                                        run_data["status"] = "success"
                                        break
                                    elif "fail" in aria.lower() or "x" in sel:
                                        run_data["status"] = "failure"
                                        break
                            except:
                                continue
                        
                        if "status" not in run_data:
                            run_data["status"] = "unknown"
                            
                    except Exception as e:
                        run_data["status_error"] = str(e)
                    
                    # 尝试获取标题/提交信息
                    try:
                        title_selectors = ["h3", ".commit-message", "a.Link--primary", ".d-flex a"]
                        for sel in title_selectors:
                            try:
                                title_elem = run.locator(sel).first
                                if title_elem.count() > 0:
                                    run_data["title"] = title_elem.inner_text()[:100]
                                    break
                            except:
                                continue
                    except:
                        pass
                    
                    # 尝试获取时间
                    try:
                        time_elem = run.locator("time, relative-time").first
                        if time_elem.count() > 0:
                            run_data["time"] = time_elem.get_attribute("datetime") or time_elem.inner_text()
                    except:
                        pass
                    
                    result["runs"].append(run_data)
                    
                except Exception as e:
                    result["runs"].append({"index": i, "error": str(e)})
            
            # 获取最新运行的详细页面
            if result["runs"]:
                latest = result["runs"][0]
                print(f"\n📊 最新运行状态: {latest.get('status', 'unknown')}")
                
                # 点击第一个运行查看详情
                try:
                    first_run_link = page.locator("a[href*='/actions/runs/']").first
                    if first_run_link.count() > 0:
                        href = first_run_link.get_attribute("href")
                        if href:
                            result["latest_run_url"] = f"https://github.com{href}"
                            print(f"🔗 最新运行链接: {result['latest_run_url']}")
                except:
                    pass
            
            # 输出结果
            print("\n" + "="*50)
            print("📋 抓取结果:")
            print("="*50)
            print(json.dumps(result, indent=2, ensure_ascii=False))
            
            return result
            
        except Exception as e:
            print(f"❌ 错误: {e}")
            # 出错时截图
            try:
                page.screenshot(path="/tmp/github_actions_error.png")
                print("📸 错误截图已保存: /tmp/github_actions_error.png")
            except:
                pass
            raise
            
        finally:
            browser.close()

def scrape_specific_run(run_id):
    """抓取特定运行的详细日志"""
    
    with sync_playwright() as p:
        browser = p.chromium.launch(headless=True)
        context = browser.new_context(
            user_agent='Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36'
        )
        page = context.new_page()
        
        try:
            url = f"https://github.com/{REPO_OWNER}/{REPO_NAME}/actions/runs/{run_id}"
            print(f"🌐 访问运行详情: {url}")
            page.goto(url, wait_until="networkidle")
            page.wait_for_timeout(3000)
            
            # 截图
            page.screenshot(path=f"/tmp/github_actions_run_{run_id}.png")
            print(f"📸 截图已保存: /tmp/github_actions_run_{run_id}.png")
            
            result = {
                "run_id": run_id,
                "url": url,
                "jobs": []
            }
            
            # 查找作业状态
            job_selectors = [
                "[data-testid='job-log']",
                ".job-item",
                ".check-run-item",
                "[data-testid='check-run']"
            ]
            
            for selector in job_selectors:
                try:
                    jobs = page.locator(selector).all()
                    if jobs:
                        print(f"✅ 找到 {len(jobs)} 个作业")
                        for job in jobs:
                            try:
                                job_name = job.locator("h3, .job-name, .text-bold").first.inner_text()
                                result["jobs"].append({"name": job_name[:50]})
                            except:
                                pass
                        break
                except:
                    continue
            
            # 获取页面上的状态文本
            page_text = page.inner_text("body")
            if "succeeded" in page_text.lower() or "completed" in page_text.lower():
                result["overall_status"] = "success"
            elif "failed" in page_text.lower() or "failure" in page_text.lower():
                result["overall_status"] = "failure"
            else:
                result["overall_status"] = "unknown"
            
            print("\n" + "="*50)
            print("📋 运行详情:")
            print("="*50)
            print(json.dumps(result, indent=2, ensure_ascii=False))
            
            return result
            
        finally:
            browser.close()

if __name__ == "__main__":
    print("🔍 GitHub Actions 日志抓取工具")
    print("="*50)
    
    # 抓取最新状态
    result = scrape_latest_action()
    
    # 如果有运行记录，抓取第一个的详情
    if result and result.get("runs"):
        latest = result["runs"][0]
        if latest.get("status") == "failure":
            print("\n⚠️ 检测到失败，尝试获取详细日志...")
            # 从 URL 提取 run_id
            if result.get("latest_run_url"):
                run_id = result["latest_run_url"].split("/runs/")[-1].split("/")[0]
                if run_id.isdigit():
                    scrape_specific_run(run_id)
    
    print("\n✅ 完成!")
