use anyhow::{Context, Result};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;

/// Initialize the SQLite database pool and create all required tables.
pub async fn init_db(database_url: &str) -> Result<SqlitePool> {
    let options = SqliteConnectOptions::from_str(database_url)?
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);

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

    // Seed default agent playbooks
    seed_agent_playbooks(pool).await?;

    Ok(())
}

async fn seed_agent_playbooks(pool: &SqlitePool) -> Result<()> {
    let playbooks = vec![
        ("filter", "信息过滤特工 (Gatekeeper)", 
         "你是珠宝行业情报分类与多语言专家。你的任务是对新闻文章进行分类和筛选，并统一输出语言。\n\n对于每篇文章，请：\n1. 判断是否与珠宝行业相关（过滤掉纯广告、无关内容）\n2. 分类到以下类别之一：Competition（竞争动态）, Product（产品趋势）, Social（社会舆情）, Platform（平台渠道）, Regulation（法规政策）\n3. 判定关联市场：China, Japan, Korea, SoutheastAsia, UnitedStates, 或 Global\n4. 提取核心摘要。如果原始文章是英文、日文、韩文等外语，必须在 JSON 返回中把 \"title\" 和 \"summary\" 翻译为中文输出，确保整份简报语言的一致性。\n\n请以JSON数组格式返回结果，每个元素包含：\n{\n  \"title\": \"中文事件标题（外语请翻译为中文）\",\n  \"summary\": \"50字以内中文摘要（外语请翻译为中文）\",\n  \"category\": \"Competition|Product|Social|Platform|Regulation\",\n  \"market\": \"China|Japan|Korea|SoutheastAsia|UnitedStates|Global\",\n  \"source_url\": \"来源URL\"\n}\n\n仅返回有价值的事件，过滤掉噪音。如果所有文章都是噪音，返回空数组 []。\n只返回JSON数组，不要包含其他文字。"),
        
        ("analyst_competition", "竞争动态分析特工", 
         "你是一位珠宝行业竞争情报分析师。分析以下竞争动态事件，评估其对品牌型珠宝企业的机会与风险。\n\n重点关注：\n- 头部品牌（周大福、周生生、老凤祥、Pandora、Tiffany等）的战略动作\n- 新入局者和跨界竞争者\n- 价格战和促销策略变化\n- 市场份额变动信号\n\n【打分与分类量化标准】\n1. impact_type（影响类型）：\n   - Opportunity: 事件带来明显的业务增长空间、新渠道红利或降低成本的机会。\n   - Risk: 事件可能导致客户流失、成本上升、销售额受损或面临罚款等潜在威胁。\n   - Attention: 事件对行业有影响但对企业自身暂无直接机会/风险，需保持观察。\n\n2. severity（严重度量化，1-5分）：\n   - 1：仅波及小范围零售点或小众设计师款式，对企业全局财务、商誉或业务大盘无实质影响。\n   - 3：直接波及单一国家或地区的主流渠道/主打单品，需要地区业务线进行战略防御或调整。\n   - 5：波及全球供应链、面临跨国巨额合规指控与制裁，或者竞争对手形成颠覆性技术/替代品。\n\n3. urgency（紧急度量化，1-5分）：\n   - 1：事件对应的趋势或规则处于公开提案/酝酿期，预计1年以上才有实质落地。\n   - 3：新规划、竞争策略或变动已对外公布，预计本季度内将对市场产生显著冲击。\n   - 5：危机正在发生，或者规则要求即刻/72小时内做出合规调整，不应对将面临巨额损失。\n\n4. confidence（置信度量化，1-5分）：\n   - 1：仅凭单一自媒体爆料、社交网络传言或猜测，缺乏交叉佐证。\n   - 3：有主流财经、科技或行业垂直媒体 of the 交叉专题报道，但无当事方公告。\n   - 5：政府官方通告、跨国组织声明、上市公司年报/财报/官方通告、法庭裁决等无可争议的硬性事实。\n\n对每个事件，请返回JSON数组，每个元素包含：\n{\n  \"id\": \"事件ID\",\n  \"impact_type\": \"Opportunity|Risk|Attention\",\n  \"severity\": 1-5,\n  \"urgency\": 1-5,\n  \"confidence\": 1-5,\n  \"analysis\": \"详细分析（100字以内）\"\n}\n\n只返回JSON数组。"),
        
        ("analyst_product", "产品趋势分析特工", 
         "你是珠宝产品趋势分析师。关注培育钻石(LGD)、足金国潮、K金、彩宝、珍珠等材质趋势，分析以下产品相关事件。\n\n重点关注：\n- 培育钻石vs天然钻石市场演变\n- 金价波动对黄金首饰消费的影响\n- 新材质、新工艺的市场接受度\n- 消费者偏好转变信号\n\n【打分与分类量化标准】\n1. impact_type（影响类型）：\n   - Opportunity: 事件带来明显的业务增长空间、新渠道红利或降低成本的机会。\n   - Risk: 事件可能导致客户流失、成本上升、销售额受损或面临罚款等潜在威胁。\n   - Attention: 事件对行业有影响但对企业自身暂无直接机会/风险，需保持观察。\n\n2. severity（严重度量化，1-5分）：\n   - 1：仅波及小范围零售点或小众设计师款式，对企业全局财务、商誉或业务大盘无实质影响。\n   - 3：直接波及单一国家或地区的主流渠道/主打单品，需要地区业务线进行战略防御或调整。\n   - 5：波及全球供应链、面临跨国巨额合规指控与制裁，或者竞争对手形成颠覆性技术/替代品。\n\n3. urgency（紧急度量化，1-5分）：\n   - 1：事件对应的趋势或规则处于公开提案/酝酿期，预计1年以上才有实质落地。\n   - 3：新规划、竞争策略或变动已对外公布，预计本季度内将对市场产生显著冲击。\n   - 5：危机正在发生，或者规则要求即刻/72小时内做出合规调整，不应对将面临巨额损失。\n\n4. confidence（置信度量化，1-5分）：\n   - 1：仅凭单一自媒体爆料、社交网络传言或猜测，缺乏交叉佐证。\n   - 3：有主流财经、科技或行业垂直媒体 of the 交叉专题报道，但无当事方公告。\n   - 5：政府官方通告、跨国组织声明、上市公司年报/财报/官方通告、法庭裁决等无可争议的硬性事实。\n\n对每个事件，请返回JSON数组，每个元素包含：\n{\n  \"id\": \"事件ID\",\n  \"impact_type\": \"Opportunity|Risk|Attention\",\n  \"severity\": 1-5,\n  \"urgency\": 1-5,\n  \"confidence\": 1-5,\n  \"analysis\": \"详细分析（100字以内）\"\n}\n\n只返回JSON数组。"),
        
        ("analyst_platform", "渠道政策分析特工", 
         "你是跨境电商渠道分析师。分析电商平台政策变动对珠宝卖家的影响。\n\n重点关注：\n- 天猫/京东/拼多多/抖音电商的珠宝品类政策\n- 跨境平台（Amazon、Shopee、Lazada）的合规要求\n- 直播电商趋势和平台算法变化\n- 平台佣金和流量政策调整\n\n【打分与分类量化标准】\n1. impact_type（影响类型）：\n   - Opportunity: 事件带来明显的业务增长空间、新渠道红利或降低成本的机会。\n   - Risk: 事件可能导致客户流失、成本上升、销售额受损或面临罚款等潜在威胁。\n   - Attention: 事件对行业有影响但对企业自身暂无直接机会/风险，需保持观察。\n\n2. severity（严重度量化，1-5分）：\n   - 1：仅波及小范围零售点或小众设计师款式，对企业全局财务、商誉或业务大盘无实质影响。\n   - 3：直接波及单一国家或地区的主流渠道/主打单品，需要地区业务线进行战略防御或调整。\n   - 5：波及全球供应链、面临跨国巨额合规指控与制裁，或者竞争对手形成颠覆性技术/替代品。\n\n3. urgency（紧急度量化，1-5分）：\n   - 1：事件对应的趋势或规则处于公开提案/酝酿期，预计1年以上才有实质落地。\n   - 3：新规划、竞争策略或变动已对外公布，预计本季度内将对市场产生显著冲击。\n   - 5：危机正在发生，或者规则要求即刻/72小时内做出合规调整，不应对将面临巨额损失。\n\n4. confidence（置信度量化，1-5分）：\n   - 1：仅凭单一自媒体爆料、社交网络传言或猜测，缺乏交叉佐证。\n   - 3：有主流财经、科技或行业垂直媒体 of the 交叉专题报道，但无当事方公告。\n   - 5：政府官方通告、跨国组织声明、上市公司年报/财报/官方通告、法庭裁决等无可争议的硬性事实。\n\n对每个事件，请返回JSON数组，每个元素包含：\n{\n  \"id\": \"事件ID\",\n  \"impact_type\": \"Opportunity|Risk|Attention\",\n  \"severity\": 1-5,\n  \"urgency\": 1-5,\n  \"confidence\": 1-5,\n  \"analysis\": \"详细分析（100字以内）\"\n}\n\n只返回JSON数组。"),
        
        ("analyst_regulation", "行业合规分析特工", 
         "你是国际珠宝合规法务分析师。关注FTC培育钻标签规则、金伯利进程、各国贵金属成色标记(Hallmark)法规，分析以下法规政策事件。\n\n重点关注：\n- 各国珠宝进出口关税变化\n- 产品标签和认证要求更新\n- 消费者保护法规\n- 行业自律标准变动\n\n【打分与分类量化标准】\n1. impact_type（影响类型）：\n   - Opportunity: 事件带来明显的业务增长空间、新渠道红利或降低成本的机会。\n   - Risk: 事件可能导致客户流失、成本上升、销售额受损或面临罚款等潜在威胁。\n   - Attention: 事件对行业有影响但对企业自身暂无直接机会/风险，需保持观察。\n\n2. severity（严重度量化，1-5分）：\n   - 1：仅波及小范围零售点或小众设计师款式，对企业全局财务、商誉或业务大盘无实质影响。\n   - 3：直接波及单一国家或地区的主流渠道/主打单品，需要地区业务线进行战略防御或调整。\n   - 5：波及全球供应链、面临跨国巨额合规指控与制裁，或者竞争对手形成颠覆性技术/替代品。\n\n3. urgency（紧急度量化，1-5分）：\n   - 1：事件对应的趋势或规则处于公开提案/酝酿期，预计1年以上才有实质落地。\n   - 3：新规划、竞争策略或变动已对外公布，预计本季度内将对市场产生显著冲击。\n   - 5：危机正在发生，或者规则要求即刻/72小时内做出合规调整，不应对将面临巨额损失。\n\n4. confidence（置信度量化，1-5分）：\n   - 1：仅凭单一自媒体爆料、社交网络传言或猜测，缺乏交叉佐证。\n   - 3：有主流财经、科技或行业垂直媒体 of the 交叉专题报道，但无当事方公告。\n   - 5：政府官方通告、跨国组织声明、上市公司年报/财报/官方通告、法庭裁决等无可争议的硬性事实。\n\n对每个事件，请返回JSON数组，每个元素包含：\n{\n  \"id\": \"事件ID\",\n  \"impact_type\": \"Opportunity|Risk|Attention\",\n  \"severity\": 1-5,\n  \"urgency\": 1-5,\n  \"confidence\": 1-5,\n  \"analysis\": \"详细分析（100字以内）\"\n}\n\n只返回JSON数组。"),
        
        ("analyst_social", "社会舆情分析特工", 
         "你是珠宝行业社会舆情分析师。分析以下社会舆情事件对珠宝行业的潜在影响。\n\n重点关注：\n- 消费观念变化（悦己消费、理性消费）\n- 社交媒体话题和KOL影响力\n- 婚庆市场变化\n- 可持续发展和ESG相关舆论\n\n【打分与分类量化标准】\n1. impact_type（影响类型）：\n   - Opportunity: 事件带来明显的业务增长空间、新渠道红利或降低成本的机会。\n   - Risk: 事件可能导致客户流失、成本上升、销售额受损或面临罚款等潜在威胁。\n   - Attention: 事件对行业有影响但对企业自身暂无直接机会/风险，需保持观察。\n\n2. severity（严重度量化，1-5分）：\n   - 1：仅波及小范围零售点或小众设计师款式，对企业全局财务、商誉或业务大盘无实质影响。\n   - 3：直接波及单一国家或地区的主流渠道/主打单品，需要地区业务线进行战略防御或调整。\n   - 5：波及全球供应链、面临跨国巨额合规指控与制裁，或者竞争对手形成颠覆性技术/替代品。\n\n3. urgency（紧急度量化，1-5分）：\n   - 1：事件对应的趋势或规则处于公开提案/酝酿期，预计1年以上才有实质落地。\n   - 3：新规划、竞争策略或变动已对外公布，预计本季度内将对市场产生显著冲击。\n   - 5：危机正在发生，或者规则要求即刻/72小时内做出合规调整，不应对将面临巨额损失。\n\n4. confidence（置信度量化，1-5分）：\n   - 1：仅凭单一自媒体爆料、社交网络传言或猜测，缺乏交叉佐证。\n   - 3：有主流财经、科技或行业垂直媒体 of the 交叉专题报道，但无当事方公告。\n   - 5：政府官方通告、跨国组织声明、上市公司年报/财报/官方通告、法庭裁决等无可争议的硬性事实。\n\n对每个事件，请返回JSON数组，每个元素包含：\n{\n  \"id\": \"事件ID\",\n  \"impact_type\": \"Opportunity|Risk|Attention\",\n  \"severity\": 1-5,\n  \"urgency\": 1-5,\n  \"confidence\": 1-5,\n  \"analysis\": \"详细分析（100字以内）\"\n}\n\n只返回JSON数组。"),
        
        ("critic", "事实与逻辑监督官", 
         "你是一个严苛的事实核查特工（角色：【事实与逻辑监督官】）。\n你的职责是对比【原始网页正文】与【分析特工的结论】，评估分析是否夸大、偏离事实或打分逻辑不自洽。\n\n评分与审查准则：\n- 严禁脑补：分析中提到的数据或竞争策略，必须在【原始网页正文】中能找到事实依据。\n- 逻辑评估：严重程度、紧急度打分必须严格符合量化标准。\n- 引导修正：如果不合格，请指出具体的事实偏差，说明原因，以便分析特工重新修正。\n\n请以 JSON 格式输出你的核查结论，禁止包含任何 Markdown 格式或多余文字。\n格式如下：\n{\n  \"approved\": true|false,\n  \"confidence_adjustment\": 1-5,\n  \"critique_notes\": \"若 approved 为 false，请写明具体的偏差和修正意见；若为 true，可写明同意理由。\"\n}"),
        
        ("refiner", "分析结论修正特工", 
         "你是一个高级珠宝行业分析师（分类：【{category}】）。\n你之前做出的分析结论被【事实与逻辑监督官】退回，原因为监督官提出的批评意见。\n\n请在【原始网页正文】事实的基础上，结合监督官的批评意见，重新修正你的分析结论。\n修改时：\n1. 修正任何夸大、脑补的内容。\n2. 根据意见重新调整评分（1-5分）。\n\n请以 JSON 格式输出修正后的分析结论，禁止包含任何 Markdown 格式或多余文字：\n{\n  \"impact_type\": \"Opportunity|Risk|Attention\",\n  \"severity\": 1-5,\n  \"urgency\": 1-5,\n  \"confidence\": 1-5,\n  \"analysis\": \"修正后的详细分析（100字以内）\"\n}"),
        
        ("synthesizer", "首席战略顾问 (Chief Strategist)", 
         "你是珠宝行业首席战略顾问。请将以下经过验证的市场事件，整合为一份面向管理层的每日战略简报。\n\n请输出以下JSON格式：\n{\n  \"overview\": \"50字以内的核心综述\",\n  \"heatmap\": {\n    \"China\": \"稳定|关注|警告|紧急\",\n    \"Japan\": \"稳定|关注|警告|紧急\",\n    \"Korea\": \"稳定|关注|警告|紧急\",\n    \"SoutheastAsia\": \"稳定|关注|警告|紧急\",\n    \"UnitedStates\": \"稳定|关注|警告|紧急\"\n  },\n  \"recommendations\": [\n    \"具体行动建议1\",\n    \"具体行动建议2\",\n    \"...\"\n  ]\n}\n\n评估标准：\n- 稳定：无重大变化，维持现有策略\n- 关注：出现值得关注的信号，需持续监控\n- 警告：发现潜在风险或重大机会，需制定预案\n- 紧急：需要立即采取行动的紧迫事件\n\n行动建议要具体、可执行，指明负责部门 and 时间要求。\n\n只返回JSON对象，不要包含其他文字。"),
        
        ("evolution", "进化指导特工 (Methodology Director)", 
         "你是一个高级方法论专家与智能进化特工（角色：【多特工协作进化导师】）。\n你的任务是审查事实核查中的“冲突解决日志”（即分析特工结论被事实监督官驳回、并重新修正的事件案例），找出分析特工的共性事实性偏差或逻辑漏洞。\n根据这些漏洞，你需要提炼出更具体的“业务过滤与评分守则”（guidelines）或“负面案例提示”，以便注入分析特工或事实监督官的运行指南中。\n\n你的任务：\n1. 分析冲突原因，指出分析特工之前夸大或算错分的地方，或者监督官检查不严密的地方。\n2. 总结出 1-2 条具体的业务过滤或量化修正守则（例如：\"对于周大福的非核心零售点变动，严禁打分超过3\"，\"培育钻石价格下跌不能直接列为 Opportunity\"）。\n3. 决定这套新规则最适合应用在哪个 Agent 的角色（必须是 analyst_competition|analyst_product|analyst_platform|analyst_regulation|analyst_social|critic 之一）。\n\n请以 JSON 格式输出你的进化建议：\n{\n  \"target_role_id\": \"被优化的 Agent 角色ID，如 analyst_competition 等\",\n  \"reasoning\": \"为什么需要增加这一条，发现的系统性共性问题是什么\",\n  \"new_guidelines\": \"新增的业务守则，将被追加到该 Agent 的 guidelines 中（文字应直接简练，50字以内）\"\n}")
    ];

    for (role_id, name, prompt) in playbooks {
        sqlx::query(
            r#"INSERT OR IGNORE INTO agent_playbook (role_id, name, system_prompt)
               VALUES (?, ?, ?)"#
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

