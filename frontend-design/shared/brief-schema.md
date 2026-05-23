# AnchorMAS · Brief Data Schema (v1)

简报页（Daily Brief）的前端契约。Agent 生成结果按此结构落到 API / 静态 JSON，前端 fetch 后渲染。

参考样本：`brief-sample.json`

---

## 1. 顶层结构

```jsonc
{
  "date":          "YYYY-MM-DD",           // 该期简报对应日期
  "generated_at":  "ISO 8601 timestamp",    // Agent 跑这期的时刻

  "lead":    { /* §2 */ },
  "stories": [ /* §3，按 rank 升序 */ ]
}
```

---

## 2. `lead` — 头版 lede

```jsonc
{
  "summary":    "Five overseas markets registered material movement today. 中国、东南亚需立即关注，三起平台 / 法规变动正在重塑跨境运营节奏。",
  "highlight":  "中国、东南亚需立即关注"
}
```

| 字段 | 必填 | 说明 |
|---|---|---|
| `summary`   | ✓ | 当日开场白全文。可中英混排 |
| `highlight` | 可选 | 在 `summary` 里需要 `<em>` 陶土色加粗的子串（chunk match） |

---

## 3. `story` — 单条简报

```jsonc
{
  "id":            "2026-05-23-cn-01",
  "rank":          1,

  "region":        "cn",                      // §4 enum
  "country_focus": null,                      // 可选，多国大区下的具体国家

  "impact": {
    "category":  "risk",                      // §4 enum
    "severity":  4,                           // 1..5
    "urgency":   4                            // 1..5
  },

  "tags":     ["regulation", "platform"],     // §4 enum array

  "headline": "中国海关跨境电商出口 HS 编码申报抽查规则 6 月起升级",
  "outlook":  "出口东南亚、中东海外仓的中小品牌可能面临 3–5 日清关延迟。",
  "action":   "复核 SKU 在新规下的归类正确性 ＋ 报关代理资质审查。",

  "sources":  [ /* §5 Source 数组，1..N 条 */ ]
}
```

| 字段 | 必填 | 约束 |
|---|---|---|
| `id` | ✓ | 全局唯一稳定 string |
| `rank` | ✓ | 整数，升序展示 |
| `region` | ✓ | §4 enum |
| `country_focus` | 可选 | ISO 3166-1 alpha-2，多国大区（如 SEA）下的具体国家 |
| `impact.category` | ✓ | §4 enum |
| `impact.severity` | ✓ | 整数 1-5，chip 自动着色 |
| `impact.urgency` | ✓ | 同上 |
| `tags` | 可选 | §4 enum array |
| `headline` | ✓ | 中文一句话，≤ 40 字符 |
| `outlook` | ✓ | 业务影响判断，1-2 句 |
| `action` | ✓ | 建议行动，1-2 句 |
| `sources` | ✓ | §5 Source 数组，≥ 1 |

---

## 4. Enums

### `region`（story 用）
| 值 | 含义 |
|---|---|
| `cn` | 中国 |
| `jp` | 日本 |
| `kr` | 韩国 |
| `sea` | 东南亚 |
| `us` | 美国 |

筛选器另有伪 enum `all` —— **仅前端使用**，data 里不出现。

### `Source.market`
**Source 的 market 是 story.region 的超集**，多一个 `global`，用于地区类型模糊的新闻（如全球趋势报道、跨地区分析）。

| 值 | 含义 |
|---|---|
| `cn` / `jp` / `kr` / `sea` / `us` | 同 story.region |
| `global` | 模糊 / 跨地区 |

### `impact.category`
| 值 | UI | 颜色 |
|---|---|---|
| `risk`        | Risk        | 陶土红 `#b8472b` |
| `attention`   | Attention   | 琥珀 `#c8821e` |
| `opportunity` | Opportunity | 苔绿 `#5f7d35` |

### `tags`（赛题 5 类维度）
`competition` / `product` / `platform` / `social` / `regulation`

### `Source.lang`
ISO 639-1：`zh` / `en` / `ja` / `ko` / `vi` / `id` / `th` / `pt` …

---

## 5. `Source` — 原子可复用类型 🧱（对齐爬虫输出）

整个产品里的"信源"是**最小复用单元**，story.sources 直接嵌套这个标准体；以后追踪页、对话引用、新闻流等地方一律复用同一结构。

```jsonc
{
  "id":         "c11f6f6e-e7b8-447a-9820-1b72aa8d3fa2",   // 爬虫给的 UUID
  "url":        "https://www.mocknews.com/jewelry/...",    // 原文链接（爬虫的 SOURCE 列）
  "title":      "Chow Tai Fook 2026 Strategy Announcement", // 文章标题
  "content":    "Full article body in original language…",  // 全文
  "chars":      4523,                                       // 字符数
  "timestamp":  "2026-05-23T08:12:00+08:00",                // 完整 ISO（不再用 HH:MM）
  "lang":       "zh",                                       // §4 lang
  "market":     "global"                                    // §4 market
}
```

| 字段 | 必填 | 约束 / 备注 |
|---|---|---|
| `id`        | ✓ | 全局稳定 UUID（爬虫产出） |
| `url`       | ✓ | 完整原文 URL。对应爬虫 `SOURCE` 列 |
| `title`     | ✓ | 文章原始标题 |
| `content`   | ✓ | 文章全文。Source Viewer 直接 render |
| `chars`     | ✓ | 字符数，前端可用作阅读时长估算 |
| `timestamp` | ✓ | ISO 8601 带时区 |
| `lang`      | ✓ | §4 lang enum |
| `market`    | ✓ | §4 market enum（包含 `global`） |

### 前端处理约定

- **发布机构显示名**：爬虫 / Schema **不提供** publisher 字段。前端从 `url` 的 host 派生（如 `reuters.com` → `Reuters`，`customs.gov.cn` → `海关总署`）。维护一份 `host → name` 映射，未命中时显示 host 本身。
- **时间显示**：UI 上的 `HH:MM` 由 `timestamp` 提取；"X h ago" 用 `timestamp` 和 `Date.now()` 算。
- **复用承诺**：同一原始链接在系统内**必须共享同一 `id`**（content-addressable），便于跨页面引用 / 去重。

---

## 6. 客户端独立 state（**不由 Agent 产出**）

| localStorage key | 类型 | 用途 |
|---|---|---|
| `anchormas:brief` | `{date, region}` | 用户当前选的日期 + 地区 |
| `anchormas:tracked` | `string[]` | 已追踪的 `story.id` 列表 |
| `anchormas:news-region` | string | 新闻流地区筛选 |
| `anchormas:tab` | string | 当前 tab |

---

## 7. 版本与演进

**v1**：当前文档。Source 对齐爬虫真实输出。

**后续可扩展点**（不影响 v1 兼容）：
- `story.confidence` — Agent 置信度
- `story.delta` — 相对前一日的变化
- `story.follow_up_of` — 链到先前 `story.id`，呼应追踪功能
- `Source.publisher` — 后处理给出的发布机构名（如果将来想从后端拿，不再前端派生）
- `Source.credibility` — 信源可信度评级

新增字段一律可选；已有字段不重命名 / 不改语义。

---

## 8. 示例

完整 5 条样本见 `brief-sample.json`。
