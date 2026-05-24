use anyhow::{Context, Result};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;

/// Initialize the SQLite database pool and create all required tables.
pub async fn init_db(database_url: &str) -> Result<SqlitePool> {
    let options = SqliteConnectOptions::from_str(database_url)?
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .busy_timeout(std::time::Duration::from_secs(10))
        .pragma("foreign_keys", "on");

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await
        .context("Failed to connect to SQLite database")?;

    run_migrations(&pool).await?;

    tracing::info!("Database initialized successfully");
    Ok(pool)
}

async fn run_migrations(pool: &SqlitePool) -> Result<()> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS raw_articles (
            id          TEXT PRIMARY KEY,
            source_url  TEXT NOT NULL,
            resolved_url TEXT,
            pipeline_status TEXT,
            title       TEXT NOT NULL,
            content     TEXT NOT NULL DEFAULT '',
            raw_language TEXT NOT NULL DEFAULT 'en',
            market      TEXT NOT NULL DEFAULT 'Global',
            created_at  TEXT NOT NULL DEFAULT (datetime('now'))
        );
        "#,
    )
    .execute(pool)
    .await
    .context("Failed to create raw_articles table")?;

    // Safe migration: Add resolved_url column if it is missing
    let _ = sqlx::query("ALTER TABLE raw_articles ADD COLUMN resolved_url TEXT;")
        .execute(pool)
        .await;

    // Safe migration: Add pipeline_status column if it is missing
    let _ = sqlx::query("ALTER TABLE raw_articles ADD COLUMN pipeline_status TEXT;")
        .execute(pool)
        .await;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS events (
            id          TEXT PRIMARY KEY,
            market      TEXT NOT NULL,
            category    TEXT NOT NULL,
            title       TEXT NOT NULL,
            summary     TEXT NOT NULL DEFAULT '',
            impact_type TEXT NOT NULL DEFAULT 'Attention',
            severity    INTEGER NOT NULL DEFAULT 1,
            urgency     INTEGER NOT NULL DEFAULT 1,
            confidence  INTEGER NOT NULL DEFAULT 1,
            source_urls TEXT NOT NULL DEFAULT '[]',
            briefing_id TEXT,
            analysis    TEXT NOT NULL DEFAULT '',
            created_at  TEXT NOT NULL DEFAULT (datetime('now'))
        );
        "#,
    )
    .execute(pool)
    .await
    .context("Failed to create events table")?;

    // Safe migration: Add analysis column if it is missing
    let _ = sqlx::query("ALTER TABLE events ADD COLUMN analysis TEXT NOT NULL DEFAULT '';")
        .execute(pool)
        .await;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS briefings (
            id                   TEXT PRIMARY KEY,
            date                 TEXT NOT NULL,
            overview             TEXT NOT NULL DEFAULT '',
            heatmap_json         TEXT NOT NULL DEFAULT '{}',
            recommendations_json TEXT NOT NULL DEFAULT '[]',
            created_at           TEXT NOT NULL DEFAULT (datetime('now'))
        );
        "#,
    )
    .execute(pool)
    .await
    .context("Failed to create briefings table")?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS briefing_events (
            briefing_id TEXT NOT NULL,
            event_id    TEXT NOT NULL,
            position    INTEGER NOT NULL DEFAULT 0,
            created_at  TEXT NOT NULL DEFAULT (datetime('now')),
            PRIMARY KEY (briefing_id, event_id),
            FOREIGN KEY(briefing_id) REFERENCES briefings(id) ON DELETE CASCADE
        );
        "#,
    )
    .execute(pool)
    .await
    .context("Failed to create briefing_events table")?;

    sqlx::query(
        r#"
        INSERT OR IGNORE INTO briefing_events (briefing_id, event_id, position, created_at)
        SELECT briefing_id, id, 0, created_at
        FROM events
        WHERE briefing_id IS NOT NULL AND briefing_id <> ''
        "#,
    )
    .execute(pool)
    .await
    .context("Failed to backfill briefing_events table")?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS chat_history (
            id           TEXT PRIMARY KEY,
            briefing_id  TEXT NOT NULL,
            user_message TEXT NOT NULL,
            ai_response  TEXT NOT NULL,
            created_at   TEXT NOT NULL DEFAULT (datetime('now'))
        );
        "#,
    )
    .execute(pool)
    .await
    .context("Failed to create chat_history table")?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS data_sources (
            id          TEXT PRIMARY KEY,
            url         TEXT NOT NULL UNIQUE,
            source_type TEXT NOT NULL,
            language    TEXT NOT NULL DEFAULT 'en',
            is_active   INTEGER NOT NULL DEFAULT 1,
            created_at  TEXT NOT NULL DEFAULT (datetime('now'))
        );
        "#,
    )
    .execute(pool)
    .await
    .context("Failed to create data_sources table")?;

    // Seed default data sources if table is empty
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM data_sources")
        .fetch_one(pool)
        .await
        .unwrap_or((0,));

    if count.0 == 0 {
        let defaults = vec![
            ("https://www.jckonline.com/feed/", "rss", "en"),
            ("https://news.google.com/rss/search?q=jewelry+industry&hl=en", "rss", "en"),
            ("https://news.google.com/rss/search?q=珠宝+行业&hl=zh-CN", "rss", "zh"),
            ("https://news.google.com/rss/search?q=ジュエリー+業界&hl=ja", "rss", "ja"),
            ("https://news.google.com/rss/search?q=주얼리+산업&hl=ko", "rss", "ko"),
            ("https://www.reddit.com/r/jewelry/new.json?limit=10", "reddit", "en"),
        ];

        for (url, source_type, lang) in defaults {
            let id = uuid::Uuid::new_v4().to_string();
            let _ = sqlx::query(
                "INSERT INTO data_sources (id, url, source_type, language) VALUES (?, ?, ?, ?)"
            )
            .bind(&id)
            .bind(url)
            .bind(source_type)
            .bind(lang)
            .execute(pool)
            .await;
        }
    }

    // Add indexes for optimization
    sqlx::query(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_raw_articles_source_url ON raw_articles (source_url);"
    )
    .execute(pool)
    .await
    .context("Failed to create index idx_raw_articles_source_url")?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_events_briefing_id ON events (briefing_id);"
    )
    .execute(pool)
    .await
    .context("Failed to create index idx_events_briefing_id")?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_briefing_events_briefing_id ON briefing_events (briefing_id);"
    )
    .execute(pool)
    .await
    .context("Failed to create index idx_briefing_events_briefing_id")?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_briefing_events_event_id ON briefing_events (event_id);"
    )
    .execute(pool)
    .await
    .context("Failed to create index idx_briefing_events_event_id")?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_chat_history_briefing_id ON chat_history (briefing_id);"
    )
    .execute(pool)
    .await
    .context("Failed to create index idx_chat_history_briefing_id")?;

    // Create Agent Playbook & Evolution tables
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS agent_playbook (
            role_id      TEXT PRIMARY KEY,
            name         TEXT NOT NULL,
            system_prompt TEXT NOT NULL,
            guidelines   TEXT NOT NULL DEFAULT '',
            version      INTEGER NOT NULL DEFAULT 1,
            updated_at   TEXT NOT NULL DEFAULT (datetime('now'))
        );
        "#,
    )
    .execute(pool)
    .await
    .context("Failed to create agent_playbook table")?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS agent_evolution_log (
            id             TEXT PRIMARY KEY,
            role_id        TEXT NOT NULL,
            old_guidelines TEXT NOT NULL,
            new_guidelines TEXT NOT NULL,
            reasoning      TEXT NOT NULL,
            created_at     TEXT NOT NULL DEFAULT (datetime('now'))
        );
        "#,
    )
    .execute(pool)
    .await
    .context("Failed to create agent_evolution_log table")?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS agent_feedback_log (
            id          TEXT PRIMARY KEY,
            sender      TEXT NOT NULL,
            receiver    TEXT NOT NULL,
            event_id    TEXT,
            feedback    TEXT NOT NULL,
            is_resolved INTEGER NOT NULL DEFAULT 0,
            created_at  TEXT NOT NULL DEFAULT (datetime('now'))
        );
        "#,
    )
    .execute(pool)
    .await
    .context("Failed to create agent_feedback_log table")?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS agent_parliament_registry (
            role_id          TEXT PRIMARY KEY,
            status           TEXT NOT NULL DEFAULT 'active',
            sponsor_role_id  TEXT,
            tasks_completed  INTEGER NOT NULL DEFAULT 0,
            tasks_failed     INTEGER NOT NULL DEFAULT 0,
            tasks_completed_last_dist INTEGER NOT NULL DEFAULT 0,
            token_cost       INTEGER NOT NULL DEFAULT 0,
            compute_credits  INTEGER NOT NULL DEFAULT 100000,
            faction          TEXT NOT NULL DEFAULT 'Neutral',
            created_at       TEXT NOT NULL DEFAULT (datetime('now')),
            last_active_at   TEXT NOT NULL DEFAULT (datetime('now')),
            last_evolved_at  TEXT NOT NULL DEFAULT (datetime('now')),
            FOREIGN KEY(role_id) REFERENCES agent_playbook(role_id) ON DELETE CASCADE
        );
        "#,
    )
    .execute(pool)
    .await
    .context("Failed to create agent_parliament_registry table")?;

    // Migration: ensure tasks_completed_last_dist exists for existing databases
    let _ = sqlx::query(
        "ALTER TABLE agent_parliament_registry ADD COLUMN tasks_completed_last_dist INTEGER NOT NULL DEFAULT 0;"
    )
    .execute(pool)
    .await;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS parliament_proposals (
            id               TEXT PRIMARY KEY,
            proposer_role_id TEXT NOT NULL,
            title            TEXT NOT NULL,
            description      TEXT NOT NULL,
            proposal_type    TEXT NOT NULL,
            status           TEXT NOT NULL DEFAULT 'voting',
            yes_votes        REAL NOT NULL DEFAULT 0,
            no_votes         REAL NOT NULL DEFAULT 0,
            created_at       TEXT NOT NULL DEFAULT (datetime('now')),
            resolved_at      TEXT
        );
        "#,
    )
    .execute(pool)
    .await
    .context("Failed to create parliament_proposals table")?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS parliament_votes (
            proposal_id      TEXT NOT NULL,
            voter_role_id    TEXT NOT NULL,
            vote             TEXT NOT NULL,
            weight           REAL NOT NULL,
            reason           TEXT,
            created_at       TEXT NOT NULL DEFAULT (datetime('now')),
            PRIMARY KEY (proposal_id, voter_role_id),
            FOREIGN KEY(proposal_id) REFERENCES parliament_proposals(id) ON DELETE CASCADE
        );
        "#,
    )
    .execute(pool)
    .await
    .context("Failed to create parliament_votes table")?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS parliament_ledger (
            id               TEXT PRIMARY KEY,
            event_type       TEXT NOT NULL,
            role_id          TEXT,
            details          TEXT NOT NULL,
            created_at       TEXT NOT NULL DEFAULT (datetime('now'))
        );
        "#,
    )
    .execute(pool)
    .await
    .context("Failed to create parliament_ledger table")?;

    // Create Regression Test Suite & Structured Rules tables
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS regression_test_suite (
            event_id    TEXT PRIMARY KEY,
            category    TEXT NOT NULL,
            title       TEXT NOT NULL,
            summary     TEXT NOT NULL,
            analysis    TEXT NOT NULL,
            created_at  TEXT NOT NULL DEFAULT (datetime('now'))
        );
        "#,
    )
    .execute(pool)
    .await
    .context("Failed to create regression_test_suite table")?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS agent_playbook_rules (
            rule_id     TEXT PRIMARY KEY,
            role_id     TEXT NOT NULL,
            content     TEXT NOT NULL,
            reasoning   TEXT,
            version     INTEGER NOT NULL DEFAULT 1,
            status      TEXT NOT NULL DEFAULT 'active',
            created_at  TEXT NOT NULL DEFAULT (datetime('now')),
            FOREIGN KEY(role_id) REFERENCES agent_playbook(role_id) ON DELETE CASCADE
        );
        "#,
    )
    .execute(pool)
    .await
    .context("Failed to create agent_playbook_rules table")?;

    // Seed/backfill the registry with the default/existing agents and 6 factions
    sqlx::query(
        r#"
        INSERT OR IGNORE INTO agent_parliament_registry (role_id, status, faction)
        SELECT role_id, 'active', 
               CASE 
                   WHEN role_id IN ('analyst_competition', 'filter') THEN 'Efficiency'
                   WHEN role_id IN ('analyst_product', 'designer') THEN 'Creativity'
                   WHEN role_id IN ('critic', 'analyst_regulation') THEN 'Prudence'
                   WHEN role_id IN ('refiner', 'analyst_platform') THEN 'Quality'
                   WHEN role_id IN ('synthesizer', 'analyst_social') THEN 'Agile'
                   ELSE 'Neutral'
               END
        FROM agent_playbook;
        "#,
    )
    .execute(pool)
    .await
    .context("Failed to backfill agent_parliament_registry")?;

    // Update existing agents to the new 6-faction distribution
    let _ = sqlx::query(
        r#"UPDATE agent_parliament_registry SET faction = 
           CASE 
               WHEN role_id IN ('analyst_competition', 'filter') THEN 'Efficiency'
               WHEN role_id IN ('analyst_product', 'designer') THEN 'Creativity'
               WHEN role_id IN ('critic', 'analyst_regulation') THEN 'Prudence'
               WHEN role_id IN ('refiner', 'analyst_platform') THEN 'Quality'
               WHEN role_id IN ('synthesizer', 'analyst_social') THEN 'Agile'
               ELSE 'Neutral'
           END"#
    )
    .execute(pool)
    .await;

    // Backfill agent_playbook_rules from existing agent_playbook.guidelines
    if let Ok(playbooks) = sqlx::query_as::<_, (String, String, i64)>(
        "SELECT role_id, guidelines, version FROM agent_playbook WHERE guidelines IS NOT NULL AND guidelines <> ''"
    )
    .fetch_all(pool)
    .await {
        for (role_id, guidelines, version) in playbooks {
            if let Ok(count) = sqlx::query_as::<_, (i64,)>(
                "SELECT COUNT(*) FROM agent_playbook_rules WHERE role_id = ?"
            )
            .bind(&role_id)
            .fetch_one(pool)
            .await {
                if count.0 == 0 {
                    for line in guidelines.lines() {
                        let trimmed = line.trim();
                        if trimmed.is_empty() {
                            continue;
                        }
                        let content = if trimmed.starts_with("- ") {
                            &trimmed[2..]
                        } else if trimmed.starts_with('-') {
                            &trimmed[1..]
                        } else {
                            trimmed
                        };
                        if content.is_empty() {
                            continue;
                        }
                        let rule_id = uuid::Uuid::new_v4().to_string();
                        let _ = sqlx::query(
                            "INSERT INTO agent_playbook_rules (rule_id, role_id, content, version, status) VALUES (?, ?, ?, ?, 'active')"
                        )
                        .bind(&rule_id)
                        .bind(&role_id)
                        .bind(content)
                        .bind(version)
                        .execute(pool)
                        .await;
                    }
                }
            }
        }
    }

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS bookmarks (
            id          TEXT PRIMARY KEY,
            event_id    TEXT NOT NULL,
            keywords    TEXT NOT NULL DEFAULT '[]',
            created_at  TEXT NOT NULL DEFAULT (datetime('now')),
            FOREIGN KEY(event_id) REFERENCES events(id) ON DELETE CASCADE
        );
        "#,
    )
    .execute(pool)
    .await
    .context("Failed to create bookmarks table")?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS bookmark_evidence_chain (
            id                TEXT PRIMARY KEY,
            bookmark_id       TEXT NOT NULL,
            matched_event_id  TEXT NOT NULL,
            direction         TEXT NOT NULL,
            match_score       REAL NOT NULL,
            match_reason      TEXT NOT NULL,
            created_at        TEXT NOT NULL DEFAULT (datetime('now')),
            FOREIGN KEY(bookmark_id) REFERENCES bookmarks(id) ON DELETE CASCADE,
            FOREIGN KEY(matched_event_id) REFERENCES events(id) ON DELETE CASCADE,
            UNIQUE(bookmark_id, matched_event_id)
        );
        "#,
    )
    .execute(pool)
    .await
    .context("Failed to create bookmark_evidence_chain table")?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_bookmark_event_id ON bookmarks (event_id);"
    )
    .execute(pool)
    .await
    .context("Failed to create index idx_bookmark_event_id")?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_evidence_chain_bookmark_id ON bookmark_evidence_chain (bookmark_id);"
    )
    .execute(pool)
    .await
    .context("Failed to create index idx_evidence_chain_bookmark_id")?;

    // ── App Frontend Tables ─────────────────────────────────────────────

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS chat_sessions (
            id            TEXT PRIMARY KEY,
            title         TEXT NOT NULL DEFAULT '',
            context_type  TEXT NOT NULL DEFAULT 'free',
            context_id    TEXT,
            created_at    TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at    TEXT NOT NULL DEFAULT (datetime('now'))
        );
        "#,
    )
    .execute(pool)
    .await
    .context("Failed to create chat_sessions table")?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS chat_messages (
            id          TEXT PRIMARY KEY,
            session_id  TEXT NOT NULL,
            role        TEXT NOT NULL,
            content     TEXT NOT NULL,
            created_at  TEXT NOT NULL DEFAULT (datetime('now')),
            FOREIGN KEY(session_id) REFERENCES chat_sessions(id) ON DELETE CASCADE
        );
        "#,
    )
    .execute(pool)
    .await
    .context("Failed to create chat_messages table")?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_chat_messages_session_id ON chat_messages (session_id);"
    )
    .execute(pool)
    .await
    .context("Failed to create index idx_chat_messages_session_id")?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS user_settings (
            key        TEXT PRIMARY KEY,
            value      TEXT NOT NULL DEFAULT '',
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        "#,
    )
    .execute(pool)
    .await
    .context("Failed to create user_settings table")?;

    // Seed default rows if empty
    let settings_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM user_settings")
        .fetch_one(pool)
        .await
        .unwrap_or((0,));
    if settings_count.0 == 0 {
        let _ = sqlx::query(
            "INSERT INTO user_settings (key, value) VALUES ('keywords', '[]')"
        )
        .execute(pool)
        .await;
        let _ = sqlx::query(
            "INSERT INTO user_settings (key, value) VALUES ('benchmark_companies', '[]')"
        )
        .execute(pool)
        .await;
    }

    // Seed default agent playbooks
    seed_agent_playbooks(pool).await?;

    Ok(())
}

async fn seed_agent_playbooks(pool: &SqlitePool) -> Result<()> {
    let playbooks = vec![
        ("filter", "信息过滤特工 (Gatekeeper)", 
         r#"你是珠宝行业情报分类与多语言专家。你的任务是对新闻文章进行分类和筛选，并统一输出语言。

对于每篇文章，请：
1. 判断是否与珠宝行业相关（过滤掉纯广告、无关内容）
2. 进行领域分类：
   - 经典类别：Competition（竞争动态）, Product（产品趋势）, Social（社会舆情）, Platform（平台渠道）, Regulation（法规政策）
   - 专业精细细分领域：若该事件属于经典五类之外的其他高度专精细分领域（如：宏观经济 MacroEconomy, 绿色可持续 ESG, 奢侈品并购 MergerAcquisition, 独立设计与IP联名 DesignIP, 拍卖会与收藏 Auction, 工艺技术与数字化 TechCraft 等），可以输出自定义的驼峰命名英文类别名（仅限单个单词或以拼音/英文组成的驼峰命名，限制在20字符以内，禁止使用中文类别名）。
3. 判定关联市场：China, Japan, Korea, SoutheastAsia, UnitedStates, 或 Global
4. 提取核心摘要。如果原始文章是外语，必须在 JSON 返回中把 "title" 和 "summary" 翻译为中文输出，确保整份简报语言的一致性。

请以JSON数组格式返回结果，每个元素包含：
{
  "title": "中文事件标题（外语请翻译为中文）",
  "summary": "50字以内中文摘要（外语请翻译为中文）",
  "category": "Competition|Product|Social|Platform|Regulation|或者自定义驼峰英文类别",
  "market": "China|Japan|Korea|SoutheastAsia|UnitedStates|Global",
  "source_url": "来源URL"
}

仅返回有价值的事件，过滤掉噪音。如果所有文章都是噪音，返回空数组 []。
只返回JSON数组，不要包含其他文字。"#),
        
        ("analyst_competition", "竞争动态分析特工", 
         r#"你是一位珠宝行业竞争情报分析师。分析以下竞争动态事件，评估其对品牌型珠宝企业的机会与风险。

重点关注：
- 头部品牌（周大福、周生生、老凤祥、Pandora、Tiffany等）的战略动作
- 新入局者和跨界竞争者
- 价格战和促销策略变化
- 市场份额变动信号

【打分与分类量化标准】
1. impact_type（影响类型）：
   - Opportunity: 事件带来明显的业务增长空间、新渠道红利或降低成本的机会。
   - Risk: 事件可能导致客户流失、成本上升、销售额受损或面临罚款等潜在威胁。
   - Attention: 事件对行业有影响但对企业自身暂无直接机会/风险，需保持观察。

2. severity（严重度量化，1-5分）：
   - 1：仅波及小范围零售点或小众设计师款式，对企业全局财务、商誉或业务大盘无实质影响。
   - 3：直接波及单一国家或地区的主流渠道/主打单品，需要地区业务线进行战略防御或调整。
   - 5：波及全球供应链、面临跨国巨额合规指控与制裁，或者竞争对手形成颠覆性技术/替代品。

3. urgency（紧急度量化，1-5分）：
   - 1：事件对应的趋势或规则处于公开提案/酝酿期，预计1年以上才有实质落地。
   - 3：新规划、竞争策略或变动已对外公布，预计本季度内将对市场产生显著冲击。
   - 5：危机正在发生，或者规则要求即刻/72小时内做出合规调整，不应对将面临巨额损失。

4. confidence（置信度量化，1-5分）：
   - 1：仅凭单一自媒体爆料、社交网络传言或猜测，缺乏交叉佐证。
   - 3：有主流财经、科技或行业垂直媒体 of the 交叉专题报道，但无当事方公告。
   - 5：政府官方通告、跨国组织声明、上市公司年报/财报/官方通告、法庭裁决等无可争议的硬性事实。

对每个事件，请返回JSON数组，每个元素包含：
{
  "id": "事件ID",
  "impact_type": "Opportunity|Risk|Attention",
  "severity": 1-5,
  "urgency": 1-5,
  "confidence": 1-5,
  "analysis": "详细分析（100字以内）"
}

只返回JSON数组。"#),
        
        ("analyst_product", "产品趋势分析特工", 
         r#"你是珠宝产品趋势分析师。关注培育钻石(LGD)、足金国潮、K金、彩宝、珍珠等材质趋势，分析以下产品相关事件。

重点关注：
- 培育钻石vs天然钻石市场演变
- 金价波动对黄金首饰消费的影响
- 新材质、新工艺的市场接受度
- 消费者偏好转变信号

【打分与分类量化标准】
1. impact_type（影响类型）：
   - Opportunity: 事件带来明显的业务增长空间、新渠道红利或降低成本的机会。
   - Risk: 事件可能导致客户流失、成本上升、销售额受损或面临罚款等潜在威胁。
   - Attention: 事件对行业有影响但对企业自身暂无直接机会/风险，需保持观察。

2. severity（严重度量化，1-5分）：
   - 1：仅波及小范围零售点或小众设计师款式，对企业全局财务、商誉或业务大盘无实质影响。
   - 3：直接波及单一国家或地区的主流渠道/主打单品，需要地区业务线进行战略防御或调整。
   - 5：波及全球供应链、面临跨国巨额合规指控与制裁，或者竞争对手形成颠覆性技术/替代品。

3. urgency（紧急度量化，1-5分）：
   - 1：事件对应的趋势或规则处于公开提案/酝酿期，预计1年以上才有实质落地。
   - 3：新规划、竞争策略或变动已对外公布，预计本季度内将对市场产生显著冲击。
   - 5：危机正在发生，或者规则要求即刻/72小时内做出合规调整，不应对将面临巨额损失。

4. confidence（置信度量化，1-5分）：
   - 1：仅凭单一自媒体爆料、社交网络传言或猜测，缺乏交叉佐证。
   - 3：有主流财经、科技或行业垂直媒体 of the 交叉专题报道，但无当事方公告。
   - 5：政府官方通告、跨国组织声明、上市公司年报/财报/官方通告、法庭裁决等无可争议的硬性事实。

对每个事件，请返回JSON数组，每个元素包含：
{
  "id": "事件ID",
  "impact_type": "Opportunity|Risk|Attention",
  "severity": 1-5,
  "urgency": 1-5,
  "confidence": 1-5,
  "analysis": "详细分析（100字以内）"
}

只返回JSON数组。"#),
        
        ("analyst_platform", "渠道政策分析特工", 
         r#"你是跨境电商渠道分析师。分析电商平台政策变动对珠宝卖家的影响。

重点关注：
- 天猫/京东/拼多多/抖音电商的珠宝品类政策
- 跨境平台（Amazon、Shopee、Lazada）的合规要求
- 直播电商趋势 and 平台算法变化
- 平台佣金和流量政策调整

【打分与分类量化标准】
1. impact_type（影响类型）：
   - Opportunity: 事件带来明显的业务增长空间、新渠道红利或降低成本的机会。
   - Risk: 事件可能导致客户流失、成本上升、销售额受损或面临罚款等潜在威胁。
   - Attention: 事件对行业有影响但对企业自身暂无直接机会/风险，需保持观察。

2. severity（严重度量化，1-5分）：
   - 1：仅波及小范围零售点或小众设计师款式，对企业全局财务、商誉或业务大盘无实质影响。
   - 3：直接波及单一国家或地区的主流渠道/主打单品，需要地区业务线进行战略防御或调整。
   - 5：波及全球供应链、面临跨国巨额合规指控与制裁，或者竞争对手形成颠覆性技术/替代品。

3. urgency（紧急度量化，1-5分）：
   - 1：事件对应的趋势或规则处于公开提案/酝酿期，预计1年以上才有实质落地。
   - 3：新规划、竞争策略或变动已对外公布，预计本季度内将对市场产生显著冲击。
   - 5：危机正在发生，或者规则要求即刻/72小时内做出合规调整，不应对将面临巨额损失。

4. confidence（置信度量化，1-5分）：
   - 1：仅凭单一自媒体爆料、社交网络传言或猜测，缺乏交叉佐证。
   - 3：有主流财经、科技或行业垂直媒体 of the 交叉专题报道，但无当事方公告。
   - 5：政府官方通告、跨国组织声明、上市公司年报/财报/官方通告、法庭裁决等无可争议的硬性事实。

对每个事件，请返回JSON数组，每个元素包含：
{
  "id": "事件ID",
  "impact_type": "Opportunity|Risk|Attention",
  "severity": 1-5,
  "urgency": 1-5,
  "confidence": 1-5,
  "analysis": "详细分析（100字以内）"
}

只返回JSON数组。"#),
        
        ("analyst_regulation", "行业合规分析特工", 
         r#"你是国际珠宝合规法务分析师。关注FTC培育钻标签规则、金伯利进程、各国贵金属成色标记(Hallmark)法规，分析以下法规政策事件。

重点关注：
- 各国珠宝进出口关税变化
- 产品标签 and 认证要求更新
- 消费者保护法规
- 行业自律标准变动

【打分与分类量化标准】
1. impact_type（影响类型）：
   - Opportunity: 事件带来明显的业务增长空间、新渠道红利或降低成本的机会。
   - Risk: 事件可能导致客户流失、成本上升、销售额受损或面临罚款等潜在威胁。
   - Attention: 事件对行业有影响但对企业自身暂无直接机会/风险，需保持观察。

2. severity（严重度量化，1-5分）：
   - 1：仅波及小范围零售点或小众设计师款式，对企业全局财务、商誉或业务大盘无实质影响。
   - 3：直接波及单一国家或地区的主流渠道/主打单品，需要地区业务线进行战略防御或调整。
   - 5：波及全球供应链、面临跨国巨额合规指控与制裁，或者竞争对手形成颠覆性技术/替代品。

3. urgency（紧急度量化，1-5分）：
   - 1：事件对应的趋势或规则处于公开提案/酝酿期，预计1年以上才有实质落地。
   - 3：新规划、竞争策略或变动已对外公布，预计本季度内将对市场产生显著冲击。
   - 5：危机正在发生，或者规则要求即刻/72小时内做出合规调整，不应对将面临巨额损失。

4. confidence（置信度量化，1-5分）：
   - 1：仅凭单一自媒体爆料、社交网络传言或猜测，缺乏交叉佐证。
   - 3：有主流财经、科技或行业垂直媒体 of the 交叉专题报道，但无当事方公告。
   - 5：政府官方通告、跨国组织声明、上市公司年报/财报/官方通告、法庭裁决等无可争议的硬性事实。

对每个事件，请返回JSON数组，每个元素包含：
{
  "id": "事件ID",
  "impact_type": "Opportunity|Risk|Attention",
  "severity": 1-5,
  "urgency": 1-5,
  "confidence": 1-5,
  "analysis": "详细分析（100字以内）"
}

只返回JSON数组。"#),
        
        ("analyst_social", "社会舆情分析特工", 
         r#"你是珠宝行业社会舆情分析师。分析以下社会舆情事件对珠宝行业的潜在影响。

重点关注：
- 消费观念变化（悦己消费、理性消费）
- 社交媒体话题 and KOL影响力
- 婚庆市场变化
- 可持续发展 and ESG相关舆论

【打分与分类量化标准】
1. impact_type（影响类型）：
   - Opportunity: 事件带来明显的业务增长空间、新渠道红利或降低成本的机会。
   - Risk: 事件可能导致客户流失、成本上升、销售额受损或面临罚款等潜在威胁。
   - Attention: 事件对行业有影响但对企业自身暂无直接机会/风险，需保持观察。

2. severity（严重度量化，1-5分）：
   - 1：仅波及小范围零售点或小众设计师款式，对企业全局财务、商誉或业务大盘无实质影响。
   - 3：直接波及单一国家或地区的主流渠道/主打单品，需要地区业务线进行战略防御或调整。
   - 5：波及全球供应链、面临跨国巨额合规指控与制裁，或者竞争对手形成颠覆性技术/替代品。

3. urgency（紧急度量化，1-5分）：
   - 1：事件对应的趋势或规则处于公开提案/酝酿期，预计1年以上才有实质落地。
   - 3：新规划、竞争策略或变动已对外公布，预计本季度内将对市场产生显著冲击。
   - 5：危机正在发生，或者规则要求即刻/72小时内做出合规调整，不应对将面临巨额损失。

4. confidence（置信度量化，1-5分）：
   - 1：仅凭单一自媒体爆料、社交网络传言或猜测，缺乏交叉佐证。
   - 3：有主流财经、科技或行业垂直媒体 of the 交叉专题报道，但无当事方公告。
   - 5：政府官方通告、跨国组织声明、上市公司年报/财报/官方通告、法庭裁决等无可争议的硬性事实。

对每个事件，请返回JSON数组，每个元素包含：
{
  "id": "事件ID",
  "impact_type": "Opportunity|Risk|Attention",
  "severity": 1-5,
  "urgency": 1-5,
  "confidence": 1-5,
  "analysis": "详细分析（100字以内）"
}

只返回JSON数组。"#),
        
        ("critic", "事实与逻辑监督官", 
         r#"你是一个严苛的事实核查特工（角色：【事实与逻辑监督官】）。
你的职责是对比【原始网页正文】与【分析特工的结论】，评估分析是否夸大、偏离事实或打分逻辑不自洽。

评分与审查准则：
- 严禁脑补：分析中提到的数据或竞争策略，必须在【原始网页正文】中能找到事实依据。
- 逻辑评估：严重程度、紧急度打分必须严格符合量化标准。
- 引导修正：如果不合格，请指出具体的事实偏差，说明原因，以便分析特工重新修正。

请以 JSON 格式输出你的核查结论，禁止包含任何 Markdown 格式或多余文字。
格式如下：
{
  "approved": true|false,
  "confidence_adjustment": 1-5,
  "critique_notes": "若 approved 为 false，请写明具体的偏差和修正意见；若为 true，可写明同意理由。"
}"#),
        
        ("refiner", "分析结论修正特工", 
         r#"你是一个高级珠宝行业分析师（分类：【{category}】）。
你之前做出的分析结论被【事实与逻辑监督官】退回，原因为监督官提出的批评意见。

请在【原始网页正文】事实的基础上，结合监督官的批评意见，重新修正你的分析结论.
修改时：
1. 修正任何夸大、脑补的内容。
2. 根据意见重新调整评分（1-5分）。

请以 JSON 格式输出修正后的分析结论，禁止包含任何 Markdown 格式或多余文字：
{
  "impact_type": "Opportunity|Risk|Attention",
  "severity": 1-5,
  "urgency": 1-5,
  "confidence": 1-5,
  "analysis": "修正后的详细分析（100字以内）"
}"#),
        
        ("synthesizer", "首席战略顾问 (Chief Strategist)", 
         r#"你是珠宝行业首席战略顾问。请将以下经过验证的市场事件，按不同地区市场，整合为一份面向管理层的每日战略简报。

请输出以下JSON格式：
{
  "overview": {
    "Global": {
      "summary": "全球珠宝行业宏观战略综述，包含今日核心趋势、价格波动及市场影响的深度分析，字数在150-250字之间。",
      "keywords": [
        {
          "word": "核心关键词或短句（如：金价历史新高）",
          "explanation": "对该关键词/句在此刻市场环境下的商业逻辑和深远战略解释，字数在50-100字之间。",
          "event_ids": ["与此关键词相关的具体新闻/事件ID (UUID)，必须从下方给出的事件列表中获取，可包含多个ID。若无直接对应事件则留空数组。"]
        }
      ]
    },
    "China": {
      "summary": "中国珠宝市场的详细战略总结，分析宏观面、国潮趋势、主要品牌动态等。注意：必须依据今日传入的该市场事件列表进行实质总结，字数在150-250字之间。只有当传入该市场的事件列表为空时，才能且必须写‘今日无重大事件’。",
      "keywords": [
        {
          "word": "关键词或短句",
          "explanation": "深度战略释义，字数在50-100字之间。",
          "event_ids": ["对应事件的ID (UUID)"]
        }
      ]
    },
    "Japan": {
      "summary": "日本珠宝市场的详细战略总结。注意：必须依据今日传入的该市场事件列表进行实质总结，字数在150-250字之间。只有当传入该市场的事件列表为空时，才能且必须写‘今日无重大事件’。",
      "keywords": [
        {
          "word": "关键词或短句",
          "explanation": "深度战略释义，字数在50-100字之间。",
          "event_ids": ["对应事件的ID (UUID)"]
        }
      ]
    },
    "Korea": {
      "summary": "韩国珠宝市场的详细战略总结。注意：必须依据今日传入的该市场事件列表进行实质总结，字数在150-250字之间。只有当传入该市场的事件列表为空时，才能且必须写‘今日无重大事件’。",
      "keywords": [
        {
          "word": "关键词或短句",
          "explanation": "深度战略释义，字数在50-100字之间。",
          "event_ids": ["对应事件的ID (UUID)"]
        }
      ]
    },
    "SoutheastAsia": {
      "summary": "东南亚珠宝市场的详细战略总结。注意：必须依据今日传入的该市场事件列表进行实质总结，字数在150-250字之间。只有当传入该市场的事件列表为空时，才能且必须写‘今日无重大事件’。",
      "keywords": [
        {
          "word": "关键词或短句",
          "explanation": "深度战略释义，字数在50-100字之间。",
          "event_ids": ["对应事件的ID (UUID)"]
        }
      ]
    },
    "UnitedStates": {
      "summary": "美国珠宝市场的详细战略总结。注意：必须依据今日传入的该市场事件列表进行实质总结，字数在150-250字之间。只有当传入该市场的事件列表为空时，才能且必须写‘今日无重大事件’。",
      "keywords": [
        {
          "word": "关键词或短句",
          "explanation": "深度战略释义，字数在50-100字之间。",
          "event_ids": ["对应事件的ID (UUID)"]
        }
      ]
    }
  },
  "heatmap": {
    "China": {
      "status": "稳定|关注|警告|紧急",
      "notes": "状态维持或变化的核心原因，15字以内"
    },
    "Japan": {
      "status": "稳定|关注|警告|紧急",
      "notes": "核心原因，15字以内"
    },
    "Korea": {
      "status": "稳定|关注|警告|紧急",
      "notes": "核心原因，15字以内"
    },
    "SoutheastAsia": {
      "status": "稳定|关注|警告|紧急",
      "notes": "核心原因，15字以内"
    },
    "UnitedStates": {
      "status": "稳定|关注|警告|紧急",
      "notes": "核心原因，15字以内"
    }
  },
  "recommendations": [
    "针对性策略行动建议1（需具体到负责部门和执行截止时间，如：供应链部须在2日内完成...）",
    "针对性策略行动建议2",
    "..."
  ]
}

评估标准：
- 稳定：无重大变化，维持现有策略
- 关注：出现值得关注的信号，需持续监控
- 警告：发现潜在风险或重大机会，需制定预案
- 紧急：需要立即采取行动的紧迫事件

只返回JSON对象，不要包含其他任何解释性或markdown标记外层包装的文字。"#),
        
        ("evolution", "进化指导特工 (Methodology Director)", 
         r#"你是一个高级方法论专家与智能进化特工（角色：【多特工协作进化导师】）。
你的任务是审查事实核查中的“冲突解决日志”（即分析特工结论被事实监督官驳回、并重新修正的事件案例），找出分析特工的共性事实性偏差或逻辑漏洞。
根据这些漏洞，你需要提炼出更具体的“业务过滤与评分守则”（guidelines）或“负面案例提示”，以便注入分析特工或事实监督官的运行指南中。

你的任务：
1. 分析冲突原因，指出分析特工之前夸大或算错分的地方，或者监督官检查不严密的地方。
2. 总结出 1-2 条具体的业务过滤或量化修正守则（例如："对于周大福的非核心零售点变动，严禁打分超过3"，"培育钻石价格下跌不能直接列为 Opportunity"）。
3. 决定这套新规则最适合应用在哪个 Agent 的角色。这可以是核心特工（analyst_competition|analyst_product|analyst_platform|analyst_regulation|analyst_social|critic），或者是任何在冲突案例中出现的自定义分析特工角色（格式如 analyst_xxxx，请根据案例提供的数据提取对应的特工角色ID，不要输出不存在的ID）。

请以 JSON 格式输出你的进化建议：
{
  "updates": [
    {
      "target_role_id": "被优化的 Agent 角色ID，如 analyst_competition 或 analyst_policyincentive",
      "reasoning": "为什么需要增加这一条，发现的系统性共性问题是什么",
      "new_guidelines": "新增的业务守则，将被追加到该 Agent 的 guidelines 中（文字应直接简练，50字以内）"
    }
  ]
}"#),
 
        ("evidence_evaluator", "证据链评估特工", 
         r#"你是一个极其严苛、理性的行业数据分析专家。你需要判定两则新闻事件是否存在【深度的、实质性的前因后果、因果级前后进展或业务链传导关联】。

输入包括：
- 追踪方向：past（向过去溯源）或 future（向未来追踪）。
- 已收藏新闻的标题、摘要、分析、核心实体特征词与发布日期。
- 待评估候选新闻的标题、摘要、分析与发布日期。

判定标准（极其严格）：
1. 只有当待匹配候选新闻与已收藏新闻之间存在【深度的、强烈的、明确的因果演进、链条式传导或针对性的竞争应对关系】时，才判定为有实质关联 (is_linked = true)。
2. 时间与方向逻辑判定：
   - 如果追踪方向为 past（向过去溯源）：【待评估候选新闻】必须是【已收藏新闻】的直接起因、背景、或前置事件。即“因为候选新闻事件发生了，才导致了或推动了收藏新闻事件的发生”。
   - 如果追踪方向为 future（向未来追踪）：【待评估候选新闻】必须是【已收藏新闻】的直接后续进展、执行结果、或引发的反弹应对事件。即“因为收藏新闻事件发生了，才导致了候选新闻事件的发生”。
   - 如果逻辑关系与时间演进方向不符（例如候选新闻发生在未来，却被评估为起因），必须判定为无实质关联 (is_linked = false，且 match_score = 0.0)。
3. 严禁宽泛或概念关联：
   - 相同新闻/平行报道：如果两则新闻只是不同媒体对同一发生的同一个事件的重复或平行报道，或者两则新闻内容高度重合/是同一个事件的同质化描述，不能作为证据链中的因果推进步骤！必须判定为无实质关联 (is_linked = false，且 match_score = 0.0)。
   - 概念/品牌/材质关联：如果两则新闻只是泛泛地提及了相同的品牌（如都提到了周大福）、相同的贵金属材质（如都提到了黄金）、或者同属一个大行业概念但无事件级的具体因果传导，必须判定为无实质关联 (is_linked = false，且 match_score = 0.0)。

重要要求：
- 请给出精确、客观的关联度评分 (match_score)，取值范围在 0.0 到 1.0 之间。只有在关联极度紧密、因果传导极度明确时才给出 0.80 以上的分数。
- 评分请细化到两位小数（如 0.84, 0.91），避免使用 0.80、0.70 等规整或默认的数字 bias。

如果存在上述强关联，请输出 JSON 格式（禁止包含任何外层包装或 markdown 标记）：
{
  "is_linked": true,
  "match_score": 0.84,
  "relation_type": "cause|result|comparison",
  "relation_description": "用一句话（50字以内）说明两者的直接业务因果关联，必须具体说明逻辑传导机制（如：‘该品牌降价策略直接导致其毛利率在下季度财报中录得显著下滑’）"
}
如果无实质关联，请将 is_linked 设为 false，格式如下：
{
  "is_linked": false,
  "match_score": 0.0,
  "relation_type": "none",
  "relation_description": ""
}"#),

        ("designer", "智能体设计专家 (Meta-Agent Designer)",
         r#"你是一个高级智能体架构师与特工设计大师（角色：【Meta-Agent Designer】）。
你的任务是根据系统检测到的全新珠宝细分分析领域，为该领域定制设计一个专业的【分析特工（Analyst Agent）】。

设计准则：
1. 分析特工的主要职责是评估该领域内的新闻事件对珠宝企业的商业决策价值（包括判断影响类型：Opportunity/Risk/Attention，打出严重度、紧急度与置信度分数 1-5 分，并给出专业深度分析）。
2. 在系统提示词中，融入该细分珠宝领域的商业逻辑与专业分析维度（例如若领域为“Auction”，提示词应强调苏富比/佳士得拍卖成交价、彩钻投资级估值、古董珠宝流转及收藏家情绪）。
3. 必须继承统一的打分规则（Opportunity/Risk/Attention 以及 1-5 评分定义）。

请直接以 JSON 格式输出设计成果，无需 any Markdown 外壳或解释：
{
  "name": "特工名称（如：收藏与拍卖分析特工）",
  "system_prompt": "设计的完整系统提示词文本"
}"#),

        ("auditor", "简报质量审计专家",
         r#"你是一个资深的报告审计官（角色：【简报质量审计专家】）。
你的任务是审查每日战略简报的质量，看它是否符合高管决策的要求。

主要审计项：
1. 综述（overview）是否过于笼统或存在拼写/语言不统一的问题（必须全中文，且具体到各市场核心动态）。
   注意：如果今日传入该市场的实际事件数量为 0，则该市场【必须】写为“今日无重大事件”。只有当实际事件数大于 0 时，才绝对禁止写“今日无重大事件”或空动套话，而必须具体分析。
2. 热力图评级是否真实反映了事件的紧急度与严重度，是否与综述内容一致。
3. 行动建议（recommendations）是否具体、可执行，是否指明了对应的业务部门 and 时间限制（例如，不能只写“密切关注”，而应该写“营运部本周内调整定价”）。

如果发现问题，请写明具体的问题和优化建议，这些内容将被写入反馈日志，指导 Synthesizer 特工优化其 Prompt。
请以 JSON 格式输出审计结果：
{
  "approved": true|false,
  "critique_notes": "指出具体的问题（不超过100字），如果 approved 为 true 则写'合格'"
}"#)
    ];

    for (role_id, name, prompt) in playbooks {
        sqlx::query(
            r#"INSERT INTO agent_playbook (role_id, name, system_prompt)
               VALUES (?, ?, ?)
               ON CONFLICT(role_id) DO UPDATE SET system_prompt = excluded.system_prompt"#
        )
        .bind(role_id)
        .bind(name)
        .bind(prompt)
        .execute(pool)
        .await
        .context(format!("Failed to seed agent playbook for {}", role_id))?;
    }

    Ok(())
}
