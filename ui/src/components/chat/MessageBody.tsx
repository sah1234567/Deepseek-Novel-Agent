import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type { ContentBlock } from "../../hooks/useAgent";
import "./MessageBody.css";

export function MessageBody({ blocks }: { blocks: ContentBlock[] }) {
  return (
    <div className="message-body">
      {blocks.map((block) =>
        block.kind === "thinking" ? (
          <details key={block.blockIndex} className="thinking-block">
            <summary>推理过程</summary>
            <p>{block.text}</p>
          </details>
        ) : (
          <div key={block.blockIndex} className="markdown-body">
            <ReactMarkdown remarkPlugins={[remarkGfm]}>{block.text}</ReactMarkdown>
          </div>
        ),
      )}
    </div>
  );
}
