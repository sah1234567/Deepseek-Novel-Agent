import { useState, useMemo } from "react";
import type { ProjectFileEntry } from "../../hooks/useProjectFiles";
import "./FileTreePanel.css";

interface TreeNode {
  name: string;
  path: string;
  isDir: boolean;
  children: TreeNode[];
}

function buildTree(files: ProjectFileEntry[]): TreeNode[] {
  const root: TreeNode[] = [];
  const dirMap = new Map<string, TreeNode>();

  // Sort: directories first, then alphabetical
  const sorted = [...files].sort((a, b) => {
    if (a.isDir !== b.isDir) return a.isDir ? -1 : 1;
    return a.path.localeCompare(b.path);
  });

  for (const f of sorted) {
    const node: TreeNode = {
      name: f.path.split("/").pop() ?? f.path,
      path: f.path,
      isDir: f.isDir,
      children: [],
    };
    dirMap.set(f.path, node);

    const slashIdx = f.path.lastIndexOf("/");
    if (slashIdx === -1) {
      root.push(node);
    } else {
      const parentPath = f.path.slice(0, slashIdx);
      const parent = dirMap.get(parentPath);
      if (parent) {
        parent.children.push(node);
      } else {
        // Parent directory not in list (shouldn't happen with walkdir), attach to root
        root.push(node);
      }
    }
  }
  return root;
}

function TreeNodeView({
  node,
  depth,
  selectedPath,
  onSelect,
}: {
  node: TreeNode;
  depth: number;
  selectedPath: string | null;
  onSelect: (path: string, isDir: boolean) => void;
}) {
  const [expanded, setExpanded] = useState(depth < 1);
  const hasChildren = node.children.length > 0;

  return (
    <li className="tree-node">
      <button
        type="button"
        className={`tree-node-btn ${selectedPath === node.path ? "selected" : ""}`}
        style={{ paddingLeft: `${depth * 1.0 + 0.35}rem` }}
        onClick={() => {
          if (node.isDir) {
            setExpanded((v) => !v);
          }
          onSelect(node.path, node.isDir);
        }}
        title={node.path}
      >
        {hasChildren ? (
          <span className="tree-arrow">{expanded ? "▾" : "▸"}</span>
        ) : (
          <span className="tree-arrow tree-arrow-spacer" />
        )}
        <span className="tree-icon">{node.isDir ? "📁" : "📄"}</span>
        <span className="tree-name">{node.name}</span>
        {node.isDir && hasChildren && (
          <span className="tree-count">{node.children.length}</span>
        )}
      </button>
      {hasChildren && expanded && (
        <ul className="tree-children">
          {node.children.map((child) => (
            <TreeNodeView
              key={child.path}
              node={child}
              depth={depth + 1}
              selectedPath={selectedPath}
              onSelect={onSelect}
            />
          ))}
        </ul>
      )}
    </li>
  );
}

export function FileTreePanel({
  files,
  previewPath,
  loading = false,
  projectInitialized = false,
  onOpen,
  onRefresh,
  collapsed,
  onToggle,
}: {
  files: ProjectFileEntry[];
  previewPath: string | null;
  loading?: boolean;
  projectInitialized?: boolean;
  onOpen: (path: string, isDir: boolean) => void;
  onRefresh: () => void;
  collapsed: boolean;
  onToggle: () => void;
}) {
  const tree = useMemo(() => buildTree(files), [files]);

  if (collapsed) {
    return (
      <aside className="file-tree-panel collapsed">
        <button type="button" className="sidebar-toggle" onClick={onToggle} title="展开文件树">
          文件
        </button>
      </aside>
    );
  }

  return (
    <aside className="file-tree-panel">
      <div className="file-tree-header">
        <h2>项目文件</h2>
        <div className="file-tree-actions">
          <button type="button" onClick={onRefresh} title="刷新">
            ↻
          </button>
          <button type="button" onClick={onToggle} title="收起">
            ‹
          </button>
        </div>
      </div>
      <ul className="file-tree-list">
        {loading && tree.length === 0 && (
          <li className="file-tree-empty">加载项目文件中…</li>
        )}
        {!loading && tree.length === 0 && (
          <li className="file-tree-empty">
            {projectInitialized
              ? "当前作品暂无可浏览文件"
              : "请先在设置中初始化作品"}
          </li>
        )}
        {tree.map((node) => (
          <TreeNodeView
            key={node.path}
            node={node}
            depth={0}
            selectedPath={previewPath}
            onSelect={onOpen}
          />
        ))}
      </ul>
    </aside>
  );
}
