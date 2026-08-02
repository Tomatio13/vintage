import { useEffect, useMemo, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import "@fontsource-variable/sora/index.css";
import "./App.css";
import "./styles/workspace.css";
import { useAppearance } from "./appearance";
import { useFontScale } from "./fontScale";
import { useKeybindings } from "./settings/keybindings.ts";
import { host } from "./host";
import type { AppUpdateInfo, AppUpdateProgress } from "./host/types";
import {
  SettingsScreen,
  SettingsSidebar,
  type SettingsSection,
} from "./settings/Settings";
import { usesOverlayTitlebar } from "./shared/platform";
import { WorkspaceApp } from "./workspace/WorkspaceApp";
import type { AppUpdatePhase } from "./update/types";

type AppView = "session" | "settings";

export function App() {
  const {
    preference: appearance,
    resolved: resolvedAppearance,
    setPreference: setAppearance,
  } = useAppearance();
  const { fontScale, setFontScale } = useFontScale();
  const { bindings, bind, resetAll } = useKeybindings();
  const overlayTitlebar = usesOverlayTitlebar();
  const [activeView, setActiveView] = useState<AppView>("session");
  const [activeSettingsSection, setActiveSettingsSection] =
    useState<SettingsSection>("application");
  const [appVersion, setAppVersion] = useState<string | null>(null);
  const [appUpdate, setAppUpdate] = useState<AppUpdateInfo | null>(null);
  const [updatePhase, setUpdatePhase] = useState<AppUpdatePhase>("idle");
  const [updateProgress, setUpdateProgress] =
    useState<AppUpdateProgress | null>(null);
  const [updateError, setUpdateError] = useState<string | null>(null);
  const [updateNotice, setUpdateNotice] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    void getVersion().then((version) => {
      if (!cancelled) setAppVersion(version);
    });
    void host.updates
      .check()
      .then((info) => {
        if (!cancelled) setAppUpdate(info);
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    let disposed = false;
    void host.updates.onProgress((payload) => {
      if (disposed) return;
      setUpdateProgress(payload);
      if (payload.stage === "downloading") setUpdatePhase("downloading");
    });
    return () => {
      disposed = true;
    };
  }, []);

  const checkForUpdates = () => {
    setUpdatePhase("checking");
    void host.updates
      .check()
      .then((info) => {
        setUpdatePhase("idle");
        setAppUpdate(info);
        setUpdateNotice(
          info
            ? `Version ${info.version} is available.`
            : "You are up to date.",
        );
      })
      .catch((error: unknown) => {
        setUpdatePhase("error");
        setUpdateError(String(error));
      });
  };

  const installUpdate = () => {
    void host.updates
      .install()
      .then(() => setUpdatePhase("idle"))
      .catch((error: unknown) => {
        setUpdatePhase("error");
        setUpdateError(String(error));
      });
  };

  const openSettings = (section: SettingsSection) => {
    setActiveSettingsSection(section);
    setActiveView("settings");
  };

  const shell = useMemo(
    () => (
      <WorkspaceApp
        appearance={resolvedAppearance}
        active={activeView === "session"}
        bindings={bindings}
        onOpenSettings={() => openSettings("application")}
      />
    ),
    [resolvedAppearance, activeView, bindings],
  );

  return (
    <div
      className={`app-shell ${overlayTitlebar ? "has-overlay-titlebar" : ""} ${activeView === "settings" ? "settings-open" : "terminal-shell"}`}
      data-tauri-drag-region={overlayTitlebar ? "deep" : undefined}
    >
      <main
        className="workspace workspace-session"
        hidden={activeView !== "session"}
      >
        {shell}
      </main>
      {activeView === "settings" && (
        <>
          <SettingsSidebar
            overlayTitlebar={overlayTitlebar}
            section={activeSettingsSection}
            onSectionChange={setActiveSettingsSection}
            onBack={() => setActiveView("session")}
          />
          <main className="workspace">
            <SettingsScreen
              overlayTitlebar={overlayTitlebar}
              section={activeSettingsSection}
              appearance={appearance}
              fontScale={fontScale}
              bindings={bindings}
              appVersion={appVersion}
              update={appUpdate}
              updatePhase={updatePhase}
              updateProgress={updateProgress}
              updateNotice={updateNotice}
              updateError={updateError}
              onCheckForUpdates={checkForUpdates}
              onAppearanceChange={setAppearance}
              onFontScaleChange={setFontScale}
              onBindKey={bind}
              onResetKeybindings={resetAll}
              onInstallUpdate={installUpdate}
            />
          </main>
        </>
      )}
    </div>
  );
}

export default App;
