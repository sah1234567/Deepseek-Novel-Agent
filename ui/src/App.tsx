import { useEffect, useRef, useState } from "react";
import { ChatPanel } from "./components/chat/ChatPanel";
import { ErrorBanner } from "./components/layout/ErrorBanner";
import { FileTreePanel } from "./components/layout/FileTreePanel";
import { SettingsPanel } from "./components/SettingsPanel";
import { StatusBar } from "./components/StatusBar";
import { APP_DISPLAY_NAME } from "./constants/app";
import { AgentProvider, useAgentContext } from "./context/AgentContext";
import { useAppStatus } from "./hooks/useAppStatus";
import { useProjectFiles } from "./hooks/useProjectFiles";
import { GraphCanvas } from "./graph/GraphCanvas";
import { GraphHitlModal } from "./graph/GraphHitlModal";
import { GraphPanelOverlay } from "./graph/GraphPanelOverlay";
import { NodeSessionHeader } from "./graph/NodeSessionHeader";
import { useGraphState } from "./graph/useGraphState";
import { ReactFlowProvider } from "@xyflow/react";

function AppShell({
  appStatus,
}: {
  appStatus: ReturnType<typeof useAppStatus>;
}) {
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [fileTreeCollapsed, setFileTreeCollapsed] = useState(false);
  const [errorDismissed, setErrorDismissed] = useState(false);
  const [transcriptBootstrapError, setTranscriptBootstrapError] = useState<string | null>(
    null,
  );
  const [statusBarError, setStatusBarError] = useState<string | null>(null);

  const {
    status,
    error,
    refresh,
    initProject,
    setPermissionMode,
    setInteractionMode,
    resumeSession,
    createSession,
    createWork,
    openWork,
    listWorks,
    listSessions,
    getApiConfig,
    setApiConfig,
    updateSessionTodo,
  } = appStatus;

  const agent = useAgentContext();
  const graph = useGraphState();
  const projectFiles = useProjectFiles(
    status?.projectInitialized ?? false,
    status?.activeWorkName,
  );
  const fileScrollCache = useRef(new Map<string, number>());
  const currentFileScrollRef = useRef(0);
  const prevPreviewPathRef = useRef<string | null>(null);

  const previewOpen = !!(projectFiles.previewPath && projectFiles.previewContent !== null);
  const subAgentOverlayOpen = !!agent.openForkRunId;
  const openForkRun = agent.openForkRunId
    ? agent.forkRuns.get(agent.openForkRunId)
    : undefined;
  const overlayActive = previewOpen || subAgentOverlayOpen;

  useEffect(() => {
    document.title = APP_DISPLAY_NAME;
  }, []);

  useEffect(() => {
    const prevPath = prevPreviewPathRef.current;
    const nextPath = projectFiles.previewPath;
    if (prevPath && prevPath !== nextPath) {
      fileScrollCache.current.set(prevPath, currentFileScrollRef.current);
    }
    prevPreviewPathRef.current = nextPath;
  }, [projectFiles.previewPath]);

  function closeFilePreview() {
    if (projectFiles.previewPath) {
      fileScrollCache.current.set(projectFiles.previewPath, currentFileScrollRef.current);
    }
    projectFiles.clearPreview();
  }

  const bannerError = errorDismissed
    ? null
    : error ??
      agent.questionError ??
      projectFiles.error ??
      transcriptBootstrapError ??
      statusBarError ??
      graph.error;

  const sessionNode = graph.sessionNode;
  const sessionSnap = graph.snapshot;
  const hasPlan = !!graph.snapshot?.hasPlan;

  return (
    <div className="app">
      <header className="app-header">
        <h1>{APP_DISPLAY_NAME}</h1>
      </header>
      <ErrorBanner
        message={bannerError}
        onDismiss={() => {
          setErrorDismissed(true);
          agent.clearQuestionError();
        }}
      />
      <StatusBar
        status={status}
        isStreaming={agent.isStreaming}
        listWorks={listWorks}
        listSessions={listSessions}
        onOpenWork={async (name) => {
          await openWork(name);
          graph.backToChat();
          graph.closeGraph();
          setErrorDismissed(false);
        }}
        onCreateWork={async (name) => {
          await createWork(name);
          graph.backToChat();
          graph.closeGraph();
          setErrorDismissed(false);
        }}
        onResumeSession={async (sessionId) => {
          try {
            await resumeSession(sessionId);
            setErrorDismissed(false);
          } catch {
            setErrorDismissed(false);
          }
        }}
        onOpenSettings={() => setSettingsOpen(true)}
        onNewSession={async () => {
          try {
            await createSession();
            await refresh();
            setErrorDismissed(false);
          } catch {
            // error shown via ErrorBanner
          }
        }}
        onCycleTodo={async (todoId, nextStatus) => {
          const sessionId = status?.sessionId;
          if (!sessionId) return;
          try {
            await updateSessionTodo(todoId, nextStatus, sessionId);
          } catch (e) {
            setStatusBarError(String(e));
            setErrorDismissed(false);
          }
        }}
        onSessionError={(message) => {
          setStatusBarError(message);
          setErrorDismissed(false);
        }}
        onOpenGraph={() => {
          void graph.backToChat().finally(() => {
            graph.openGraph();
          });
        }}
        onSetInteractionMode={async (mode) => {
          try {
            if (mode === "orchestrate") {
              await graph.backToChat();
            } else {
              await setInteractionMode(mode);
            }
            await refresh();
            setErrorDismissed(false);
          } catch {
            setErrorDismissed(false);
          }
        }}
        graphHitlCount={graph.snapshot?.hitl?.length ?? status?.graphHitlCount ?? 0}
        hasPlan={hasPlan}
      />
      <main className="app-main">
        <FileTreePanel
          files={projectFiles.files}
          previewPath={projectFiles.previewPath}
          loading={projectFiles.loading}
          projectInitialized={status?.projectInitialized ?? false}
          onOpen={(path, isDir) => void projectFiles.openFile(path, isDir)}
          onRefresh={() => void projectFiles.refresh()}
          collapsed={fileTreeCollapsed}
          onToggle={() => setFileTreeCollapsed((v) => !v)}
        />
        <div className="main-stage">
          {sessionNode && sessionSnap ? (
            <NodeSessionHeader
              node={sessionNode}
              snapshot={sessionSnap}
              onBack={() => void graph.backToChat()}
              onStart={() => void graph.start(sessionNode.id)}
              onApprove={() => void graph.approve(sessionNode.id)}
              onReject={() => void graph.reject(sessionNode.id, "needs revision")}
              onReopen={() => void graph.reopen(sessionNode.id)}
            />
          ) : null}
          <div className="main-stage-chat">
            <ChatPanel
              permissionMode={status?.permissionMode ?? "normal"}
              sessionId={status?.sessionId}
              appTurnInProgress={status?.turnInProgress ?? false}
              onSetPermissionMode={setPermissionMode}
              overlayActive={overlayActive}
              filePreview={
                previewOpen &&
                projectFiles.previewPath &&
                projectFiles.previewContent !== null
                  ? {
                      path: projectFiles.previewPath,
                      content: projectFiles.previewContent,
                      initialScrollTop:
                        fileScrollCache.current.get(projectFiles.previewPath) ?? 0,
                      onScrollPositionChange: (top) => {
                        currentFileScrollRef.current = top;
                      },
                      onClose: closeFilePreview,
                    }
                  : null
              }
              subAgentForkRun={subAgentOverlayOpen ? openForkRun : undefined}
              onCloseSubAgent={agent.closeForkOverlay}
              onTranscriptBootstrapError={setTranscriptBootstrapError}
            />
          </div>
          {graph.graphPanelOpen ? (
            <GraphPanelOverlay onClose={graph.closeGraph}>
              <ReactFlowProvider>
                <GraphCanvas graph={graph} />
              </ReactFlowProvider>
            </GraphPanelOverlay>
          ) : null}
        </div>
      </main>
      {graph.showHitlModal && graph.snapshot ? (
        <GraphHitlModal
          items={graph.snapshot.hitl}
          onOpenGraph={graph.openGraph}
          onApprove={(id) => void graph.approve(id)}
          onReject={(id) => void graph.reject(id, "needs revision")}
          onDismiss={graph.dismissHitlModal}
        />
      ) : null}
      <SettingsPanel
        open={settingsOpen}
        onClose={() => setSettingsOpen(false)}
        projectInitialized={status?.projectInitialized ?? false}
        sessionId={status?.sessionId ?? ""}
        onInitProject={initProject}
        onResumeSession={resumeSession}
        listSessions={listSessions}
        onGetApiConfig={getApiConfig}
        onSetApiConfig={setApiConfig}
      />
    </div>
  );
}

export default function App() {
  const appStatus = useAppStatus();
  return (
    <AgentProvider onTurnComplete={(s) => void appStatus.refresh(s)}>
      <AppShell appStatus={appStatus} />
    </AgentProvider>
  );
}
