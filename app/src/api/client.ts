// 空字符串 = 走相对路径 (dev 用 vite proxy 兜 CORS)
// 非空 = 直连远端 (Capacitor / 配好 CORS 的部署)
const BASE = import.meta.env.VITE_API_BASE ?? "";

export class ApiHttpError extends Error {
  status: number;
  body: unknown;
  constructor(status: number, body: unknown, message?: string) {
    super(message ?? `HTTP ${status}`);
    this.status = status;
    this.body = body;
  }
}

export type QueryValue = string | number | undefined | null;
export type QueryMap = Record<string, QueryValue>;

interface ReqOptions extends RequestInit {
  query?: QueryMap;
}

function buildUrl(path: string, query?: QueryMap): string {
  const cleanPath = path.startsWith("/") ? path : `/${path}`;
  let qs = "";
  if (query) {
    const params = new URLSearchParams();
    for (const [k, v] of Object.entries(query)) {
      if (v === undefined || v === null || v === "") continue;
      params.set(k, String(v));
    }
    const s = params.toString();
    if (s) qs = `?${s}`;
  }
  if (!BASE) return cleanPath + qs; // 相对路径
  return BASE.replace(/\/+$/, "") + cleanPath + qs;
}

export async function api<T>(path: string, opts: ReqOptions = {}): Promise<T> {
  const { query, headers, body, ...rest } = opts;
  const url = buildUrl(path, query);
  const init: RequestInit = {
    ...rest,
    headers: {
      "Content-Type": "application/json",
      ...(headers ?? {}),
    },
    body,
  };

  const res = await fetch(url, init);

  if (res.status === 204) return undefined as T;

  const text = await res.text();
  let parsed: unknown = undefined;
  if (text) {
    try {
      parsed = JSON.parse(text);
    } catch {
      parsed = text;
    }
  }

  if (!res.ok) {
    const msg =
      parsed && typeof parsed === "object" && "error" in parsed
        ? String((parsed as { error: unknown }).error)
        : `HTTP ${res.status}`;
    throw new ApiHttpError(res.status, parsed, msg);
  }

  return parsed as T;
}

// 专用：blob (TTS)
export async function apiBlob(path: string, opts: ReqOptions = {}): Promise<Blob> {
  const { query, headers, body, ...rest } = opts;
  const url = buildUrl(path, query);
  const res = await fetch(url, {
    ...rest,
    headers: {
      "Content-Type": "application/json",
      ...(headers ?? {}),
    },
    body,
  });
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new ApiHttpError(res.status, text, `HTTP ${res.status}`);
  }
  return res.blob();
}

export { BASE as API_BASE };
