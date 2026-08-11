# AnchorMAS Agent 前端对接 API 文档

> **Base URL**: `<APP_API_BASE_URL>`
>
> **Content-Type**: `application/json`
>
> **最后更新**: 2026-05-23

---

## 目录

1. [新闻 (News)](#1-新闻-news)
2. [简报 (Briefings)](#2-简报-briefings)
3. [对话 (Chat)](#3-对话-chat)
4. [收藏 & 链路追踪 (Bookmarks)](#4-收藏--链路追踪-bookmarks)
5. [设置 (Settings)](#5-设置-settings)
6. [语音合成 TTS](#6-语音合成-tts)
7. [枚举值参考](#7-枚举值参考)
8. [错误处理](#8-错误处理)

---

## 1. 新闻 (News)

### 1.1 获取新闻列表

```
GET /app/news
```

**Query 参数**:

| 参数 | 类型 | 必填 | 默认值 | 说明 |
|------|------|------|--------|------|
| `market` | string | 否 | - | 市场筛选，见[枚举值](#market-市场) |
| `category` | string | 否 | - | 分类筛选，见[枚举值](#category-分类) |
| `page` | number | 否 | 1 | 页码，从 1 开始 |
| `size` | number | 否 | 20 | 每页条数，最大 100 |

**请求示例**:
```
GET /app/news?market=China&page=1&size=10
```

**响应**:
```json
{
  "items": [
    {
      "id": "uuid-string",
      "title": "周大福2024年Q3净利润同比下降15%",
      "summary": "受金价波动和消费疲软影响...",
      "market": "China",
      "category": "Competition",
      "impact_type": "Risk",
      "severity": 4,
      "urgency": 3,
      "confidence": 5,
      "source_urls": ["https://example.com/article1"],
      "analysis": "周大福业绩下滑反映了...",
      "created_at": "2026-05-23 08:00:00"
    }
  ],
  "total": 42,
  "page": 1,
  "size": 10
}
```

---

### 1.2 获取新闻详情

```
GET /app/news/:id
```

**路径参数**: `id` — 新闻事件 UUID

**响应**:
```json
{
  "id": "uuid-string",
  "title": "周大福2024年Q3净利润同比下降15%",
  "summary": "受金价波动和消费疲软影响...",
  "market": "China",
  "category": "Competition",
  "impact_type": "Risk",
  "severity": 4,
  "urgency": 3,
  "confidence": 5,
  "source_urls": ["https://example.com/article1"],
  "analysis": "周大福业绩下滑反映了...",
  "created_at": "2026-05-23 08:00:00",
  "raw_sources": [
    {
      "title": "原文标题",
      "source_url": "https://example.com/article1",
      "content": "原文正文内容..."
    }
  ]
}
```

> `raw_sources` 包含原始抓取的文章全文，用于展示新闻详情页。

---

## 2. 简报 (Briefings)

### 2.1 获取简报列表

```
GET /app/briefings
```

**Query 参数**:

| 参数 | 类型 | 必填 | 默认值 | 说明 |
|------|------|------|--------|------|
| `page` | number | 否 | 1 | 页码 |
| `size` | number | 否 | 10 | 每页条数 |

**响应**:
```json
{
  "items": [
    {
      "id": "uuid-string",
      "date": "2026-05-23",
      "overview": { /* JSON 概要数据，前端自行渲染 */ },
      "created_at": "2026-05-23 08:00:00"
    }
  ],
  "total": 5,
  "page": 1,
  "size": 10
}
```

---

### 2.2 获取最新简报详情

```
GET /app/briefings/latest
```

**Query 参数**:

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `market` | string | 否 | 按市场筛选关联事件 |

**响应**:
```json
{
  "id": "uuid-string",
  "date": "2026-05-23",
  "overview": { /* JSON 概要 */ },
  "heatmap": { /* 热力图 JSON */ },
  "recommendations": [ /* 建议数组 */ ],
  "events": [
    {
      "id": "event-uuid",
      "title": "...",
      "summary": "...",
      "market": "China",
      "category": "Competition",
      "impact_type": "Risk",
      "severity": 4,
      "urgency": 3,
      "confidence": 5,
      "source_urls": ["..."],
      "analysis": "...",
      "created_at": "2026-05-23 08:00:00"
    }
  ],
  "created_at": "2026-05-23 08:00:00"
}
```

---

### 2.3 获取指定简报详情

```
GET /app/briefings/:id
```

**Query 参数**: 同 2.2（`market` 可选）

**响应格式**: 同 2.2

---

## 3. 对话 (Chat)

### 3.1 获取所有会话

```
GET /app/chat/sessions
```

**响应**:
```json
[
  {
    "id": "session-uuid",
    "title": "关于周大福降价策略的讨论",
    "context_type": "news",
    "context_id": "event-uuid-or-null",
    "created_at": "2026-05-23 08:00:00",
    "updated_at": "2026-05-23 09:30:00"
  }
]
```

---

### 3.2 创建会话

```
POST /app/chat/sessions
```

**请求体**:
```json
{
  "title": "可选标题",
  "context_type": "free",
  "context_id": null
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `title` | string | 否 | 不传则自动生成 |
| `context_type` | string | 否 | `"free"` / `"news"` / `"briefing"`，默认 `"free"` |
| `context_id` | string | 否 | 关联的新闻ID或简报ID，`free` 模式下为 null |

**context_type 说明**:
- `"free"` — 自由对话，无额外上下文
- `"news"` — @新闻对话，传入 `context_id` 为新闻事件 ID，AI 会自动注入该新闻的标题/摘要/分析/原文作为上下文
- `"briefing"` — @简报对话，传入 `context_id` 为简报 ID，AI 会自动注入简报概要、热力图、建议和关联事件

**响应**: `201 Created`
```json
{
  "id": "session-uuid",
  "title": "自由对话",
  "context_type": "free",
  "context_id": null,
  "created_at": "2026-05-23 08:00:00",
  "updated_at": "2026-05-23 08:00:00"
}
```

---

### 3.3 获取会话消息历史

```
GET /app/chat/sessions/:id/messages
```

**响应**:
```json
[
  {
    "id": "msg-uuid",
    "session_id": "session-uuid",
    "role": "user",
    "content": "周大福和老凤祥哪个更有投资价值？",
    "created_at": "2026-05-23 08:01:00"
  },
  {
    "id": "msg-uuid-2",
    "session_id": "session-uuid",
    "role": "assistant",
    "content": "从近期市场数据来看...",
    "created_at": "2026-05-23 08:01:05"
  }
]
```

---

### 3.4 发送消息

```
POST /app/chat/sessions/:id/messages
```

**请求体**:
```json
{
  "message": "周大福最近的市场表现如何？"
}
```

**响应**:
```json
{
  "user_message": {
    "id": "msg-uuid",
    "session_id": "session-uuid",
    "role": "user",
    "content": "周大福最近的市场表现如何？",
    "created_at": "2026-05-23 08:01:00"
  },
  "ai_message": {
    "id": "msg-uuid-2",
    "session_id": "session-uuid",
    "role": "assistant",
    "content": "根据最新的市场情报...",
    "created_at": "2026-05-23 08:01:05"
  }
}
```

> **注意**: 该接口是同步调用 LLM 的，响应时间通常为 2-10 秒。前端建议加 loading 状态。
>
> AI 会自动注入以下上下文：
> - 会话关联的新闻/简报原文（如果 context_type 不是 free）
> - RAG 检索到的相关历史事件（向量数据库）
> - 最近 20 条对话历史

---

### 3.5 删除会话

```
DELETE /app/chat/sessions/:id
```

**响应**: `204 No Content`

---

## 4. 收藏 & 链路追踪 (Bookmarks)

### 4.1 获取收藏列表

```
GET /app/bookmarks
```

**响应**:
```json
[
  {
    "id": "bookmark-uuid",
    "event_id": "event-uuid",
    "title": "周大福Q3净利润下降15%",
    "summary": "受金价波动影响...",
    "market": "China",
    "category": "Competition",
    "keywords": ["周大福", "净利润", "金价"],
    "evidence_count": 3,
    "created_at": "2026-05-23 08:00:00"
  }
]
```

> `evidence_count` 为关联的证据链事件数量（不含当前事件本身）

---

### 4.2 创建收藏

```
POST /app/bookmarks
```

**请求体**:
```json
{
  "event_id": "event-uuid"
}
```

**响应**: `201 Created`
```json
{
  "id": "bookmark-uuid",
  "event_id": "event-uuid",
  "title": "周大福Q3净利润下降15%",
  "summary": "受金价波动影响...",
  "market": "China",
  "category": "Competition",
  "keywords": ["周大福", "净利润", "金价"],
  "evidence_count": 0,
  "created_at": "2026-05-23 08:00:00"
}
```

> **注意**: 创建收藏时会自动调用 LLM 提取关键词。如果该事件已被收藏，返回已有的收藏记录（`200 OK`）。
>
> 收藏创建后，后台会异步执行**链路回溯**（5级递归溯源），自动关联历史相关新闻。

---

### 4.3 删除收藏

```
DELETE /app/bookmarks/:id
```

**响应**: `204 No Content`

---

### 4.4 获取收藏详情 & 证据链

```
GET /app/bookmarks/:id/chain
```

**响应**:
```json
{
  "bookmark": {
    "id": "bookmark-uuid",
    "event_id": "event-uuid",
    "title": "周大福Q3净利润下降15%",
    "summary": "受金价波动影响...",
    "market": "China",
    "category": "Competition",
    "keywords": ["周大福", "净利润", "金价"],
    "evidence_count": 3,
    "created_at": "2026-05-23 08:00:00"
  },
  "chain": [
    {
      "event_id": "past-event-1",
      "title": "国际金价突破历史新高",
      "summary": "受地缘政治影响...",
      "market": "Global",
      "date": "2026-04-15 10:00:00",
      "direction": "past",
      "match_score": 0.92,
      "relation_description": "金价上涨直接推高了珠宝企业的原材料成本，导致毛利率承压..."
    },
    {
      "event_id": "event-uuid",
      "title": "周大福Q3净利润下降15%",
      "summary": "受金价波动影响...",
      "market": "China",
      "date": "2026-05-10 08:00:00",
      "direction": "current",
      "match_score": 1.0,
      "relation_description": "当前关注新闻事件"
    },
    {
      "event_id": "future-event-1",
      "title": "周大福宣布战略性闭店计划",
      "summary": "将关闭20家低效门店...",
      "market": "China",
      "date": "2026-05-20 14:00:00",
      "direction": "future",
      "match_score": 0.88,
      "relation_description": "净利润持续下滑促使管理层采取成本控制措施..."
    }
  ]
}
```

**chain 字段说明**:

| 字段 | 说明 |
|------|------|
| `direction` | `"past"` 前因事件 / `"current"` 当前收藏事件 / `"future"` 后续进展 |
| `match_score` | 0-1 语义相关度分数，越高越相关 |
| `relation_description` | AI 生成的关联分析说明 |

> **链路按时间排序** (`date` ASC)，前端可用时间线组件展示因果链。

---

## 5. 设置 (Settings)

### 5.1 获取当前设置

```
GET /app/settings
```

**响应**:
```json
{
  "custom_keywords": ["培育钻石", "黄金回收"],
  "benchmark_companies": ["周大福", "Tiffany"],
  "updated_at": "2026-05-23 08:00:00"
}
```

---

### 5.2 更新设置

```
PUT /app/settings
```

**请求体**（所有字段可选，只传需要更新的）:
```json
{
  "custom_keywords": ["培育钻石", "黄金回收", "翡翠"],
  "benchmark_companies": ["周大福", "Tiffany", "Pandora"]
}
```

**响应**: 同 5.1 格式，返回更新后的完整设置

> **用途说明**:
> - `custom_keywords` — 影响新闻采集的搜索关键词
> - `benchmark_companies` — 影响简报生成时的对标公司分析

---

## 6. 语音合成 TTS

### 6.1 文字转语音

```
POST /app/tts
```

**请求体**:
```json
{
  "text": "要合成的文字内容",
  "voice": "longxiaochun_v3"
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `text` | string | 是 | 合成文本，建议单次不超过 500 字 |
| `voice` | string | 否 | 声音ID，默认 `longxiaochun_v3` |

**可用声音**:

| voice_id | 描述 |
|----------|------|
| `longxiaochun_v3` | 知识型女性（默认） |
| `longanyang` | 阳光大男孩 |
| `longxiaoxia_v3` | 冷静权威型女性 |

**响应**: `200 OK`

- **Content-Type**: `audio/mpeg`
- **Body**: 原始 MP3 二进制数据

**前端使用示例**:
```javascript
const response = await fetch('/app/tts', {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({ text: '要播放的文字' })
});

if (response.ok) {
  const blob = await response.blob();
  const audioUrl = URL.createObjectURL(blob);
  const audio = new Audio(audioUrl);
  audio.play();
}
```

> **典型用途**: 简报朗读、新闻摘要播报

---

## 7. 枚举值参考

### market (市场)

| 值 | 说明 |
|----|------|
| `Global` | 全球 |
| `China` | 中国 |
| `Japan` | 日本 |
| `Korea` | 韩国 |
| `SoutheastAsia` | 东南亚 |
| `UnitedStates` | 美国 |

### category (分类)

| 值 | 说明 |
|----|------|
| `Competition` | 竞争动态 |
| `Product` | 产品趋势 |
| `Social` | 社会消费 |
| `Platform` | 平台渠道 |
| `Regulation` | 政策法规 |

### impact_type (影响类型)

| 值 | 说明 |
|----|------|
| `Opportunity` | 机会 |
| `Risk` | 风险 |
| `Attention` | 关注 |

### severity / urgency / confidence (评分)

整数 1-5，分别代表严重性、紧迫性、置信度。

---

## 8. 错误处理

所有错误以 JSON 格式返回：

```json
{
  "error": "错误描述信息"
}
```

**HTTP 状态码**:

| 状态码 | 含义 |
|--------|------|
| `200` | 成功 |
| `201` | 创建成功 |
| `204` | 删除成功（无响应体） |
| `404` | 资源不存在 |
| `500` | 服务器内部错误 |

---

## 快速验证

```bash
# 新闻列表（中国市场）
curl ${APP_API_BASE_URL}/app/news?market=China

# 新闻详情
curl ${APP_API_BASE_URL}/app/news/{id}

# 最新简报
curl ${APP_API_BASE_URL}/app/briefings/latest

# 设置
curl ${APP_API_BASE_URL}/app/settings

# 创建对话
curl -X POST ${APP_API_BASE_URL}/app/chat/sessions \
  -H "Content-Type: application/json" \
  -d '{"context_type":"free"}'

# 发送消息
curl -X POST ${APP_API_BASE_URL}/app/chat/sessions/{session_id}/messages \
  -H "Content-Type: application/json" \
  -d '{"message":"珠宝行业最近有什么大事？"}'

# TTS
curl -X POST ${APP_API_BASE_URL}/app/tts \
  -H "Content-Type: application/json" \
  -d '{"text":"测试语音"}' \
  -o test.mp3

# 收藏
curl -X POST ${APP_API_BASE_URL}/app/bookmarks \
  -H "Content-Type: application/json" \
  -d '{"event_id":"某个事件ID"}'

# 证据链
curl ${APP_API_BASE_URL}/app/bookmarks/{bookmark_id}/chain
```
