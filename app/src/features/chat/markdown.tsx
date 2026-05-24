// 使用 react-markdown + remark-gfm 渲染 AI 回复
// 支持：标题、段落、列表、表格、横线、code、bold、italic、链接
// 保留 [N] 引用样式：post-process 时用自定义组件

import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

function transformCitations(text: string): string {
  // [N] → markdown 不识别的语法不好转；直接用一个 <span data-cite="N"> 包，但 react-markdown 不渲染 raw html
  // 简单做：保留 [N] 原样，CSS 不特殊处理。需要时再做。
  return text;
}

export function Markdown({ text }: { text: string }) {
  const src = transformCitations(text);
  return (
    <ReactMarkdown
      remarkPlugins={[remarkGfm]}
      components={{
        // 给链接默认 new tab
        a: ({ node: _node, href, children, ...rest }) => (
          <a href={href} target="_blank" rel="noopener noreferrer" {...rest}>{children}</a>
        ),
      }}
    >
      {src}
    </ReactMarkdown>
  );
}
