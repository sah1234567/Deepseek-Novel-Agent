import { useEffect, useRef, useState } from "react";
import { ChatPanel } from "./components/chat/ChatPanel";
import { ErrorBanner } from "./components/layout/ErrorBanner";
import { FileTreePanel } from "./components/layout/FileTreePanel";
import { SettingsPanel } from "./components/SettingsPanel";
import { StatusBar } from "./components/StatusBar";
import { AgentProvider, useAgentContext } from "./context/AgentContext";
import { useAppStatus } from "./hooks/useAppStatus";
import { useProjectFiles } from "./hooks/useProjectFiles";
import { APP_DISPLAY_NAME } from "./constants/app";

function AppShell({
  appStatus,
}: {
  appStatus: ReturnType<typeof useAppStatus>;
}) {
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [fileTreeCollapsed, setFileTreeCollapsed] = useState(false);
  const [errorDismissed, setErrorDismissed] = useState(false);

  const {
    status,
    error,
    refresh,
    initProject,
    setPermissionMode,
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

  useEffect(() => {
    if (status?.sessionId) {
      void agent.hydrateMessages();
    }
  }, [status?.sessionId, agent.hydrateMessages]);

  const hookActive = agent.hookRunning || (status?.hookRunning ?? false);

  const runningForkRunId = (() => {
    for (const [id, run] of agent.forkRuns) {
      if (run.status === "running") return id;
    }
    return null;
  })();

  const bannerError = errorDismissed
    ? null
    : error ?? agent.questionError ?? projectFiles.error;

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
        hookRunning={hookActive}
        activeSubAgent={agent.activeSubAgent}
        activeForkCount={agent.activeForkCount}
        runningForkRunId={runningForkRunId}
        onOpenForkOverlay={(forkRunId) => void agent.openForkOverlay(forkRunId)}
        lastTurnStats={agent.lastTurnStats}
        listWorks={listWorks}
        listSessions={listSessions}
        onOpenWork={async (name) => {
          await openWork(name);
          setErrorDismissed(false);
        }}
        onCreateWork={async (name) => {
          await createWork(name);
          setErrorDismissed(false);
        }}
        onResumeSession={async (sessionId) => {
          await resumeSession(sessionId);
          setErrorDismissed(false);
        }}
        onOpenSettings={() => setSettingsOpen(true)}
        onNewSession={() => {
          void (async () => {
            try {
              await createSession();
              await refresh();
              setErrorDismissed(false);
            } catch {
              // error shown via ErrorBanner
            }
          })();
        }}
        onCycleTodo={(todoId, nextStatus) =>
          void updateSessionTodo(todoId, nextStatus)
        }
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
        <div className="chat-panel-wrapper">
          <ChatPanel
            permissionMode={status?.permissionMode ?? "normal"}
            onSetPermissionMode={setPermissionMode}
            overlayActive={overlayActive}
            filePreview={
              previewOpen && projectFiles.previewPath && projectFiles.previewContent !== null
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
          />
        </div>
      </main>
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
    <AgentProvider onTurnComplete={() => void appStatus.refresh()}>
      <AppShell appStatus={appStatus} />
    </AgentProvider>
  );
}
