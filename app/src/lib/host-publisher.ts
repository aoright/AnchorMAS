// Source 不提供 publisher 字段，从 url host 派生显示名。

const HOST_TO_NAME: Record<string, string> = {
  "reuters.com": "Reuters",
  "bloomberg.com": "Bloomberg",
  "wsj.com": "WSJ",
  "ft.com": "Financial Times",
  "nytimes.com": "NYT",
  "nikkei.com": "日経",
  "asahi.com": "朝日新聞",
  "yna.co.kr": "연합뉴스",
  "joongang.co.kr": "중앙일보",
  "thejakartapost.com": "Jakarta Post",
  "straitstimes.com": "Straits Times",
  "channelnewsasia.com": "CNA",
  "techcrunch.com": "TechCrunch",
  "theverge.com": "The Verge",
  "scmp.com": "SCMP",
  "caixin.com": "财新",
  "21jingji.com": "21 财经",
  "yicai.com": "第一财经",
  "customs.gov.cn": "海关总署",
  "miit.gov.cn": "工信部",
};

export function hostFromUrl(url: string): string {
  try {
    const u = new URL(url);
    return u.hostname.replace(/^www\./, "");
  } catch {
    return url;
  }
}

export function publisherFromUrl(url: string): string {
  const host = hostFromUrl(url);
  if (HOST_TO_NAME[host]) return HOST_TO_NAME[host];
  // 模糊匹配二级域名
  for (const [key, name] of Object.entries(HOST_TO_NAME)) {
    if (host.endsWith(key)) return name;
  }
  return host;
}
