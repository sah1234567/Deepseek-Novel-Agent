import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type { ContentBlock } from "../../types/messages";
import "./MessageBody.css";

export function MessageBody({ blocks }: { blocks: ContentBlock[] }) {
  const textBlocks = blocks.filter((block) => block.kind === "text" && block.text);
  if (textBlocks.length === 0) return null;
  return (
    <div className="message-body">
      {textBlocks.map((block) => (
        <div key={block.blockIndex} className="markdown-body">
          <ReactMarkdown remarkPlugins={[remarkGfm]}>{block.text}</ReactMarkdown>
        </div>
      ))}
    </div>
  );
}
