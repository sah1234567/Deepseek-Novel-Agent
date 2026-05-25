import { ScrollViewport } from "../layout/ScrollViewport";
import "./FilePreviewOverlay.css";

export interface FilePreviewState {
  path: string;
  content: string;
  initialScrollTop: number;
  onScrollPositionChange: (top: number) => void;
  onClose: () => void;
}

export function FilePreviewOverlay({
  preview,
}: {
  preview: FilePreviewState;
}) {
  return (
    <div className="dialog-inner-overlay file-preview-overlay">
      <header className="dialog-inner-overlay-header">
        <span className="dialog-inner-overlay-title">📄 {preview.path}</span>
        <button
          type="button"
          className="dialog-inner-overlay-close"
          onClick={preview.onClose}
          title="关闭预览"
        >
          ✕
        </button>
      </header>
      <ScrollViewport
        className="dialog-inner-overlay-scroll"
        autoScrollDeps={[preview.content]}
        resetScrollKey={preview.path}
        initialScrollTop={preview.initialScrollTop}
        onScrollPositionChange={preview.onScrollPositionChange}
      >
        <pre className="file-preview-body">{preview.content}</pre>
      </ScrollViewport>
    </div>
  );
}
