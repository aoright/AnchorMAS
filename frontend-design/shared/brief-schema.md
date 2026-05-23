# AnchorMAS · Brief Data Schema (v1)

简报页（Daily Brief）的前端契约。Agent 生成结果按此结构落到 API / 静态 JSON，前端 fetch 后渲染。

参考样本：`brief-sample.json`

---

## 1. 顶层结构

```jsonc
{
  "date":          "YYYY-MM-DD",           // 该期简报对应日期
  "edition":       42,                      // 当年第 N 期，可选
  "generated_at":  "ISO 8601 timestamp",    // Agent 跑这期的时间

  "lead":    { /* 见 §2 */ },
  "stories": [ /* §3，按 rank 升序 */ ]
}
```

---

## 2. `lead` — 头版 lede

页面顶部"今日开场白"区域。

```jsonc
{
  "summary_en":  "Five overseas markets registered material movement today.",
  "summary_cn":  "三起平台 / 法规变动正在重塑跨境运营节奏。",
  "highlight":   "中国、东南亚需立即关注"        // <em> 陶土色加粗的片段
}
```

| 字段 | 必填 | 说明 |
|---|---|---|
| `summary_en` | ✓ | Fraunces italic 衬线英文 lede（建议 1 句，≤ 100 字符） |
| `summary_cn` | ✓ | 紧跟的中文句子 |
| `highlight`  | 可选 | 在 `summary_cn` 里需要 `<em>` 高亮的子串（chunk match） |

---

## 3. `story` — 单条简报

```jsonc
{
  "id":     "2026-05-23-cn-01",      // 稳定 ID（追踪持久化用）
  "rank":   1,                        // 显示顺序

  "region": "cn",                     // §4 enum
  "market": {
    "cn": "中国",
    "en": "China",
    "country_focus": "VN"             // 可选，region=sea 时用来标具体国家
  },

  "impact": {
    "category": "risk",               // §4 enum
    "severity": 4,                    // 整数 1..5
    "urgency":  4                     // 整数 1..5
  },

  "tags":     ["regulation", "platform"],   // §4 enum array

  "headline": "中国海关跨境电商出口 HS 编码申报抽查规则 6 月起升级",
  "outlook":  "出口东南亚、中东海外仓的中小品牌可能面临 3–5 日清关延迟。",
  "action":   "复核 SKU 在新规下的归类正确性 ＋ 报关代理资质审查。",

  "sources":  [ /* §5，1..N 条 */ ]
}
```

| 字段 | 必填 | 约束 |
|---|---|---|
| `id` | ✓ | 全局唯一稳定 string。建议格式 `YYYY-MM-DD-{region}-{seq}` 或纯 UUID |
| `rank` | ✓ | 整数，前端按升序展示 |
| `region` | ✓ | enum 见 §4，决定地区筛选行为 |
| `market.cn` | ✓ | 中文地区名 |
| `market.en` | ✓ | 英文地区名 |
| `market.country_focus` | 可选 | 大区下的具体国家（如 SEA 下的 `VN`/`ID`/`TH`），用于 UI 副标记 |
| `impact.category` | ✓ | enum 见 §4 |
| `impact.severity` | ✓ | 1=低 / 3=中 / 5=高，前端 chip 自动着色 |
| `impact.urgency` | ✓ | 同上 |
| `tags` | 可选 | enum array，当前 UI 不显示但保留分类信息（未来做维度筛选） |
| `headline` | ✓ | 中文一句话，建议 ≤ 40 字符 |
| `outlook` | ✓ | 业务影响判断，1-2 句话 |
| `action` | ✓ | 建议行动，1-2 句话 |
| `sources` | ✓ | 至少 1 条 |

---

## 4. Enums

### `region`
| 值 | 含义 |
|---|---|
| `cn` | 中国 |
| `jp` | 日本 |
| `kr` | 韩国 |
| `sea` | 东南亚（Vietnam / Indonesia / Thailand / Philippines / ...） |
| `us` | 美国 |

筛选器多了一个伪 enum `all` —— **仅前端使用**，data 里不出现。

### `impact.category`
| 值 | UI 标签 | 颜色 |
|---|---|---|
| `risk`        | Risk        | 陶土红 `#b8472b` |
| `attention`   | Attention   | 琥珀 `#c8821e` |
| `opportunity` | Opportunity | 苔绿 `#5f7d35` |

### `tags`（赛题 5 类维度）
| 值 | 含义 |
|---|---|
| `competition` | 竞争 |
| `product`     | 产品 |
| `platform`    | 平台 |
| `social`      | 社媒 |
| `regulation`  | 法规 |

每条 story 可同时归多个 tag。

### `source.lang`
ISO 639-1 双字母代码：`zh` / `en` / `ja` / `ko` / `vi` / `id` / `th` / `pt` ...

---

## 5. `source` — 原始信源

```jsonc
{
  "id":   "src-haiguan-20260523-0812",
  "name": "海关总署",
  "url":  "https://www.customs.gov.cn/...",
  "time": "08:12",
  "lang": "zh"
}
```

| 字段 | 必填 | 约束 |
|---|---|---|
| `id` | ✓ | 稳定 ID |
| `name` | ✓ | 信源原始名（中/英/日/韩文均可） |
| `url` | ✓ | 完整 URL，前端 Source Viewer 用 |
| `time` | ✓ | `HH:MM`（24h，当日发布时间） |
| `lang` | ✓ | 见 §4 |

> 前端会用 `time` + 当前时间算 "Xh ago" 显示在新闻流。

---

## 6. 客户端独立 state（**不由 Agent 产出**）

| localStorage key | 类型 | 用途 |
|---|---|---|
| `anchormas:brief` | `{date, region}` | 用户当前选的日期 + 地区 |
| `anchormas:tracked` | `string[]` | 已追踪的 `story.id` 列表 |
| `anchormas:news-region` | string | 新闻流地区筛选 |
| `anchormas:tab` | string | 当前 tab |

> ⚠ 当前 demo 里 `tracked` 临时用 `story.headline` 文本当 key。等 Agent 接好后切到 `story.id`（稳定）。

---

## 7. 版本与演进

**v1**：当前文档。

**后续可扩展点**（不影响 v1 兼容）：
- `story.confidence` — Agent 置信度（0-1）
- `story.delta` — 相对前一日的变化点（如严重度升级）
- `story.follow_up_of` — 链到先前的 `story.id`，呼应"追踪"功能
- `story.attachments[]` — 截图 / 数据可视化
- `lead.weather_index` — 整体市场温度（重启 climate strip 时用）

新增字段一律可选；已有字段不重命名 / 不改语义。

---

## 8. 示例

完整 5 条样本见 `brief-sample.json`。可直接作为后端 mock 起跑数据，或前端 fetch 校验用。
