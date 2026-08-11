# AnchorMAS · 战略模拟与市场情报多 Agent 系统

AnchorMAS 是一个多工作区（Workspace）的市场情报智能体系统，支持自主的信息采集、结构化战略分析、因果链路追踪以及人机协同问答。通过结合领域专家智能体（Analyst）、独立审计智能体（Verifier）与黑板（Blackboard）协调机制，为业务决策提供高可信度、可溯源的战略简报。

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
| `agent/` | 后端运行时 | 数据采集、Agent pipeline、自治议会、自我演化、SSE 流式 LLM 接口、`/app/*` REST API。SQLite 持久化事件/简报/会话；Qdrant 用于证据链召回 |
| `app/` | 业务团队日常使用的产品 UI | 支持移动与桌面双布局自适应，包含简报、对话、新闻、追踪与设置 |
| `frontend/` | 运维监控仪表板 | 提供 Pipeline 运行状态、事实审计痕迹、智能体进化历史与自治议会状态的可视化展示 |
| `frontend-design/` | 设计原型 | 静态 HTML 规范，作为视觉与交互的设计源头 |

---

## 解决的核心痛点

| 痛点 | AnchorMAS 的解决机制 |
|---|---|
| **市场信息高度分散** | 后端 Agent 24小时自动巡航抓取 RSS、Google News、Reddit 以及行业期刊，将分散的公开数据源归类整理 |
| **判断依赖主观经验** | Analyst Agent 根据统一的严重度、紧急度与可信度（Severity/Urgency/Confidence）指标为事件打分，确保评估标准一致 |
| **缺乏结构化决策输入** | 输出结构化为「每日战略简报」：包含市场热力扫描、核心事件卡片、今日行动建议与跨市场对比，提供清晰的决策输入 |
| **市场变化与决策时间差** | 持续拉取信源；当用户选择「追踪」某一事件时，系统能够递归召回关联历史，生成可追溯的因果证据链 |

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

### 1. 结论可追溯，事实可审计
系统引入了独立的 **Verifier Agent** 对分析结果进行事实审计与核查。Verifier Agent 会检测评分与描述的潜在冲突、无依据的过度推断以及虚构的时间约束，并在前端展示 `[核查备注]` 与 `[核查警告]`，保障决策内容的安全与可信度。

### 2. 跨时空因果追踪与证据链构建
用户点击「追踪」后，**Tracker Agent** 会在后台进行多级递归溯源：
- 结合 Qdrant 向量检索召回历史候选事件；
- 通过 LLM 分析判定候选事件与当前事件的因果关系类型（过去/当前/未来）并给出逻辑释义；
- 前端自动渲染生成一条清晰的可溯源证据链时间轴。

### 3. 多智能体黑板协作与自治演化
后端采用真正的多智能体架构，基于 **Blackboard 模式**与异步通道进行协调。同时，系统包含 **Parliament 机制**，各智能体（Analyst）拥有角色属性、生命周期状态和可审计的账本。表现不佳的智能体将被列入观察名单，系统可通过投票表决对智能体的 Playbook 规则进行修改，实现闭环演化。

### 4. 编辑式学术期刊美学设计
用户界面采用了克制且沉稳的编辑式视觉设计，融合了经典的 Serif 字体排版与中性色调，规避了传统 AI 产品的高饱和度黑盒控制台视觉，使复杂的行业资讯更易阅读。

---

## 快速启动

### 后端运行
```bash
cd agent
cp .env.example .env       # 配置您的 LLM API key 与 Qdrant 地址
cargo run                  # 默认监听 $SERVER_PORT (3000)
```

### 业务前端 `app/` 运行
```bash
cd app
npm install
npm run dev                # http://localhost:5173
```

### 运维监控前端 `frontend/` 运行
```bash
cd frontend
npm install
npm run dev                # 默认代理请求至 3000 端口
```

---

## 参考与文档

- [`API_DOC.md`](./API_DOC.md) — 前后端 API 接口协议文档
- [`CLAUDE.md`](./CLAUDE.md) — 本地开发、测试与运行的命令指南
- `agent/src/agent/blackboard.rs` — 智能体黑板编排核心
- `agent/src/agent/tracker.rs` — 因果链追溯算法
