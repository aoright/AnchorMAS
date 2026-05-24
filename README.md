# AnchorMAS · 海外市场战略情报 Agent

**赛题方向 A · 战略模拟 Agent** | **2026 AIRS Agent-For-Human 黑客松**

团队 **PandaaX** —— 刘钰恺 · 叶杰

---

## 项目定位

跨境品牌的运营团队每天面临一个具体的痛点：**6 个海外市场、5 个分析维度（竞争 / 产品 / 平台 / 社媒 / 法规）的资讯散落在几十个公开信息源里，人工扫一遍要半天，扫完之后判断标准还因人而异，关键变化经常滞后才被注意到。**

AnchorMAS 是为这个痛点而设计的 AI 战略辅助角色。在**不接入企业任何内部数据**的前提下，它 24 小时自动从公开信息源采集、过滤、分析、交叉验证，每天早晨产出一份**可溯源、可追踪、可追问**的战略简报，让业务团队晨会 5 分钟就能形成统一的决策依据。

---

## 我们解决了什么

赛题官方点出了四条核心痛点。我们逐条对应：

| 痛点 | AnchorMAS 的解法 |
|---|---|
| **市场信息高度分散** —— 新闻、报告、社群资料分散，人工查阅耗时易遗漏 | 后端 Agent 全自动 24 小时巡航 RSS · Google News · Reddit · 行业期刊，按 6 市场 × 5 维度结构化归类 |
| **判断依赖个人经验** —— 不同负责人标准不一，难以形成可对比结论 | 每条事件由 Analyst Agent 按统一三维评分（Severity / Urgency / Confidence 1-5）打分，并交叉验证；任何业务人员看到的结论标准一致 |
| **缺乏结构化决策输入** —— 资讯零散，晨会中无法快速形成明确行动方向 | 输出固化为「每日简报」结构：市场热力（status + notes）+ Top N 事件卡 + 今日行动建议 + 跨市场对照；评审"非技术可理解度"硬指标 |
| **市场变化与决策时间差** —— 法规及平台政策调整往往事后才被注意到 | 信源持续拉取 + 时间衰减召回；用户对单个事件「追踪」后，后台自动 5 级递归溯源历史，形成因果证据链时间线 |

---

## 项目结构

```text
.
├── agent/             多 Agent 后端 (Rust · Axum · SQLite · Qdrant)
├── app/               用户产品前端 (React + TypeScript)
├── frontend/          运维监控前端 (React · JSX)
└── frontend-design/   设计原型 (静态 HTML / CSS / JS)
```

| 目录 | 角色 | 说明 |
|---|---|---|
| `agent/` | 后端运行时 | 数据采集、Agent pipeline、自治议会、自我演化、SSE 流式 LLM 接口、`/app/*` REST API。SQLite 持久化事件 / 简报 / 收藏 / 会话；Qdrant 存事件向量用于证据链召回 |
| `app/` | 业务团队**每天使用**的产品 UI | 5 个 tab：简报 / 对话 / 新闻 / 追踪 / 设置。Mobile + Desktop 双布局自适应，编辑式视觉语言（Fraunces serif + 陶土红 + paper grain），暗色模式完整 |
| `frontend/` | 运维 / 演示**看 Agent 内部状态**的仪表板 | 6 个 tab：Raw Data / Pipeline / Briefing / Evidence Tracker / Agent Evolution / Agent Parliament。给开发者和评委看后台运作 |
| `frontend-design/` | 设计原型 | mobile / desktop 双端原型 + 预览页。作为 `app/` 视觉与交互的源头规范 |

`app/` 和 `frontend/` 消费同一套 `/app/*` API（见 [`API_DOC.md`](./API_DOC.md)），但形态截然不同：前者克制、给业务团队晨会用；后者高密度、给开发者看 Agent 真实运作。

---

## 系统架构

```
                    公开信息源
        RSS · Google News · Reddit · 行业期刊
                        │
                        ▼
   ┌────────────────────────────────────────────┐
   │             Agent Pipeline                 │
   │                                            │
   │  Harvester ─► Filter ─► Analyst            │
   │                            │               │
   │                            ▼               │
   │                       Verifier (核查)      │
   │                            │               │
   │                            ▼               │
   │                      Synthesizer (简报)    │
   │                                            │
   │   ⇅ Blackboard (MPSC 协调 + 限流)         │
   │   ⇅ Parliament (议会 · 评议 · 演化)        │
   └────────────────────────────────────────────┘
                        │
              SQLite + Qdrant 持久化
                        │
                        ▼
                Axum HTTP / SSE
                ┌───────┴───────┐
                ▼               ▼
           app/ (产品)      frontend/ (运维)
```

---

## 核心亮点

每条亮点先讲**对应解决了什么业务问题**，再讲**技术实现**。

### 亮点一：结论可追溯，不是 AI 黑盒

**业务问题**：评委关心、企业更关心的"AI 风险安全"——AI 给出的判断能不能溯源到原文，能不能反向审计？

**解法**：每条事件经 Analyst 打分之后，**必须强制经过独立的 Verifier Agent 做事实审计**。Verifier 不是装饰：

- 检测评分与描述冲突（"severity=1 但正文用了'重大'描述" → 反向写 feedback 给 Analyst 修正）
- 检测无依据推断（"原文未提'价值维度'，属无依据过度推断" → 标 `[核查警告]`，下游强制回溯原文锚点）
- 检测虚构时间约束（"原文全篇无任何时间节点，违反'严禁脑补'守则"）

→ 用户在简报里看到的每条结论，都已经被独立 Agent 审过；新闻详情页里把 Verifier 的批改痕迹 `[核查备注]` / `[核查警告]` 折叠隐藏但保留可读，需要时随时展开。

**技术实现**：`agent/src/agent/verifier.rs` 独立 LLM 通道；`agent/src/agent/synthesizer.rs` 在简报合成阶段二次检查，通过 `blackboard::log_feedback` 把质量问题写回 Analyst 的 playbook，下一轮 Analyst 看到反馈后改行为，形成**真正的 closed-loop 自校正**。

### 亮点二：长期跟踪一个 topic，自动构建因果链

**业务问题**：晨会简报看完即忘。但运营真正关心的是"周大福这个事 5 月发生，根因是不是 3 月那个？接下来会演化到哪里？"——人工去翻历史，永远跟不上节奏。

**解法**：用户在简报或新闻里点一下"追踪"，后台自动跑 5 级递归证据链溯源：

- Qdrant 向量库召回历史候选事件
- LLM 判断**因果关系类型**（past / current / future）+ `match_score` + `relation_description`
- 前端 `ChainTimeline` 渲染成时间线，每个节点写明 AI 推理出的因果说明

实测样例：用户追踪 2026-05-23 「中国奢侈品行业 60% 利润被 15 个品牌垄断」，系统**自动**找到 2026-03-12「老铺黄金、君佩黄金等原创高端品牌突围成功，国际奢侈品承压」，AI 写出因果说明："构成已收藏新闻所述'不高端无利润'倒逼行业分化的直接前置动因与实证案例。"

→ 这是同类产品做不到的"知识图谱级别的因果回溯"，单单这一个功能就足以构成"长期跟踪"场景的杀手锏。

**技术实现**：`agent/src/agent/tracker.rs`（Qdrant 余弦相似度召回 + LLM 关系判定） + `app/src/features/bookmarks/ChainTimeline.tsx`（时间线 UI · 节点 direction 着色 · relation_description 高亮）。

### 亮点三：统一判断标准，跨人跨市场可比

**业务问题**：传统调研报告每位分析师标准不一，今天的"高风险"和上周的"高风险"经常不是一个量级。

**解法**：固定三维评分体系（Severity / Urgency / Confidence 各 1-5），所有市场所有维度强制使用同一套量表。前端 metric chip 严格按数值色阶（1-2 苔绿 / 3 琥珀 / 4-5 陶土红），让"严重度 4"一眼可读，跨市场一眼可比。

简报顶部的 5 市场热力扫描（中国 / 日本 / 韩国 / 东南亚 / 美国）每市场一行卡，左边色条 + 右侧 status 标签 + 实质 notes，整版扫一眼就知道今天该重点关注哪两个市场。

**技术实现**：评分量表在 `agent/src/agent/analyst.rs` 的系统 prompt 里强约束；前端 `app/src/features/brief/adapters.ts` 按 `severity * urgency * 100 + confidence` 排序取 Top 8。

### 亮点四：从看简报到追问只差一步

**业务问题**：看完简报第一反应往往是"这个对我品类影响多大"——传统报告下一步只能自己查、自己想。

**解法**：每条 story / news 卡片右下角都有「追问」按钮。点击 → 自动创建带**该事件完整上下文**的对话会话 → 后端自动注入该新闻原文 + RAG 检索的历史相关事件 + 最近对话历史 → SSE 流式回答，浏览器端逐 token 渲染，支持 Markdown 标题 / 表格 / 列表 / 代码块。

会话历史持久化在 SQLite，跨设备可恢复；mobile 抽屉式会话列表、desktop sidebar 会话列表，体验对齐主流 chat 产品。

**技术实现**：`agent/src/web/app_handlers.rs::send_message` 支持 `stream: true`，输出 `text/event-stream`；`app/src/api/chat.ts::sendMessageStream` 用原生 fetch + ReadableStream 解析 SSE 帧；`app/src/hooks/useChat.ts::useStreamingSend` 维护累积文本 + 闪烁光标 + 失败回滚。

### 亮点五：真正的多 Agent 系统，不是 prompt 套娃

**业务问题**：单一 LLM prompt 容易因为"什么都做"而什么都做不好。我们要的是分工明确、彼此监督、能自我演化的协作系统。

**解法**：6 类 Agent 在 Blackboard 上协作：

- **Harvester** 抓取 RSS / Google News / Reddit / 行业期刊
- **Filter** 按珠宝行业相关性筛选并归类
- **Analyst**（多个，按 5 维度分工）打分 + 写分析
- **Verifier** 独立事实审计
- **Synthesizer** 合成最终简报
- **Tracker** 后台构建证据链

Agent 之间通过 MPSC 通道（消息：RawArticleAdded → FilteredEventAdded → AnalysisCompleted → PeerReviewCompleted → VerifierVerdict → ConsensusReached）协调，Semaphore 限制 LLM 并发，Mutex 去重事件。

**进一步**：`agent/src/agent/parliament.rs` 让 Agent 自治——每个 Analyst 有 `role_id` / `faction`（Efficiency / Creativity / Neutral）/ 生命周期状态（active / probation / parole / tombstone）/ Ledger 审计账本 / Proposal 投票。表现差的 Agent 会被停滞审计，议会裁决后通过 `evolution.rs` 的 `EvolveCrudResponse` 增删 playbook 规则，让 Agent **真正改自己的规则**而非由人改。

**技术实现**：`agent/src/agent/blackboard.rs`（核心编排）+ `parliament.rs` + `evolution.rs`。所有规则变更进 ledger 表，可审计、可回滚。

### 亮点六：克制的编辑式 UI，避免"AI 产品脸"

**业务问题**：评审"非技术可理解度"独立成项。非技术评委看不下技术黑话仪表板。

**解法**：`app/` 不走"AI 产品万年蓝紫渐变"的路线，整套美学语言贴近高质量行业期刊：

- Fraunces 可变 serif（标题）+ Schibsted Grotesk（正文）+ JetBrains Mono（标签 / 时间）+ Noto Serif SC（中文）
- 陶土红 `#b8472b` 单一品牌色，paper 灰白 `#f5f5f7` 主背景，半透 hairline 描边
- SVG fractalNoise paper grain 叠层模拟纸质
- 暗色模式完整覆盖（`[data-theme="dark"]`）

→ 评委、CEO、运营、PR 都能直接看懂、看顺眼，不被"AI 产物"的视觉成见劝退。

**技术实现**：CSS variables tokens + 原 prototype 4000 行 CSS 经 Python 脚本扫描 brace 配对后包入 `.mobile-app { ... }` / `.desktop-app { ... }` scope，避免两端 class 冲突；React Router 同一份代码根据 viewport 切 shell。

---

## 评审项对应（按官方权重）

| 评分项 | 我们的落点 |
|---|---|
| 业务落地潜力 | 「晨会 5 分钟决策」场景具象；6 市场 × 5 维度全覆盖；追踪 + 证据链让 PM 真正长期跟一个 topic |
| 技术实现 | Rust 后端 + Qdrant 向量 + 真 Blackboard 多 Agent 架构 + SSE 流式；不是 LLM wrapper |
| 风险安全机制 | Verifier 独立审计 + `[核查备注]` / `[核查警告]` 可追溯；source_urls 完整保留；议会审计账本 |
| 创新性 | Agent Parliament 自治 + Evolution 自演化规则 + Evidence Chain 因果链时间线 |
| 演示与表达 | 双视图（产品 `app/` + 监控 `frontend/`）让评委同时看 UI 美感 + Agent 内部运作 |
| 非技术可理解度 | 中文 UI 全程，编辑式美学，结构化卡片 + 状态 chip，避免技术黑话 |

---

## Demo 录屏

_（待补充）_

---

## 快速启动

### 后端

```bash
cd agent
cp .env.example .env       # 配 LLM API key / Qdrant URL
cargo run                  # 默认监听 $SERVER_PORT (3000)
```

### 产品前端 `app/`

```bash
cd app
npm install
npm run dev                # http://localhost:5173 (vite proxy 兜后端 CORS)
```

桌面（≥1024）和 mobile（<1024）双布局自适应；Capacitor 包 iOS/Android 在 roadmap 中。

### 监控前端 `frontend/`

```bash
cd frontend
npm install
npm run dev                # proxies /api → 后端 :3000
```

---

## 参考

- [`API_DOC.md`](./API_DOC.md) — 前后端 API 契约（news / briefings / chat / bookmarks / settings / tts）
- [`CLAUDE.md`](./CLAUDE.md) — 赛题原文 + 评审标准内部笔记
- `agent/src/agent/blackboard.rs` — 多 Agent 协调核心
- `agent/src/agent/tracker.rs` — 证据链算法
- `app/src/features/bookmarks/ChainTimeline.tsx` — 证据链 UI 实现

---

## 主办方

AIRS 深圳市人工智能与机器人研究院 × 玲界 OpenAgent
