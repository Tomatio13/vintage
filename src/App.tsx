import { lazy, Suspense, useEffect, useMemo, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import "@fontsource-variable/sora/index.css";
import "./App.css";
import "./styles/workspace.css";
import { useAppearance } from "./appearance";
import { useFontScale } from "./fontScale";
import { useKeybindings } from "./settings/keybindings.ts";
import { useDefaultShell } from "./settings/shells.ts";
import { useIntegrations } from "./settings/integrations.ts";
import { resolveTerminalFontFamily, useTerminalFont } from "./terminalFont.ts";
import { useTerminalScrollback } from "./terminalScrollback.ts";
import { host } from "./host";
import type { ShellDescriptor } from "./host/types";
import type { AppUpdateInfo, AppUpdateProgress } from "./host/types";
import type { SettingsSection } from "./settings/Settings";
import { usesOverlayTitlebar } from "./shared/platform";
import { WorkspaceApp } from "./workspace/WorkspaceApp";
import type { AppUpdatePhase } from "./update/types";

const SettingsScreen = lazy(async () => {
  const settings = await import("./settings/Settings");
  return { default: settings.SettingsScreen };
});
const SettingsSidebar = lazy(async () => {
  const settings = await import("./settings/Settings");
  return { default: settings.SettingsSidebar };
});

type AppView = "session" | "settings";
const STARTUP_UPDATE_DELAY_MS = 4_000;

export function App() {
  const {
    preference: appearance,
    resolved: resolvedAppearance,
    setPreference: setAppearance,
  } = useAppearance();
  const { fontScale, setFontScale } = useFontScale();
  const { bindings, bind, resetAll } = useKeybindings();
  const {
    settings: terminalFont,
    setFontSize,
    setFontFamily,
  } = useTerminalFont();
  const { scrollback, setScrollback } = useTerminalScrollback();
  const { preferredShellId, setPreferredShellId } = useDefaultShell();
  const overlayTitlebar = usesOverlayTitlebar();
  const [shells, setShells] = useState<ShellDescriptor[]>([]);
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
  const settingsApplicationOpen =
    activeView === "settings" && activeSettingsSection === "application";
  const integrations = useIntegrations(
    activeView === "settings" && activeSettingsSection === "integrations",
  );

  useEffect(() => {
    if (!settingsApplicationOpen) return;
    let cancelled = false;
    void getVersion().then((version) => {
      if (!cancelled) setAppVersion(version);
    });
    const updateTimer = window.setTimeout(() => {
      void host.updates
        .check()
        .then((info) => {
          if (!cancelled) setAppUpdate(info);
        })
        .catch(() => undefined);
    }, STARTUP_UPDATE_DELAY_MS);
    return () => {
      cancelled = true;
      window.clearTimeout(updateTimer);
    };
  }, [settingsApplicationOpen]);

  useEffect(() => {
    if (!settingsApplicationOpen) return;
    let cancelled = false;
    void host.shells
      .list()
      .then((list) => {
        if (!cancelled) setShells(list);
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [settingsApplicationOpen]);

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
        fontFamily={resolveTerminalFontFamily(terminalFont)}
        fontSize={terminalFont.size}
        scrollback={scrollback}
        preferredShellId={preferredShellId}
        onOpenSettings={() => openSettings("application")}
      />
    ),
    [resolvedAppearance, activeView, bindings, terminalFont, preferredShellId],
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
        <Suspense
          fallback={<main className="workspace">Loading settings…</main>}
        >
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
                terminalFont={terminalFont}
                terminalScrollback={scrollback}
                preferredShellId={preferredShellId}
                shells={shells}
                appVersion={appVersion}
                update={appUpdate}
                updatePhase={updatePhase}
                updateProgress={updateProgress}
                updateNotice={updateNotice}
                updateError={updateError}
                onCheckForUpdates={checkForUpdates}
                onAppearanceChange={setAppearance}
                onFontScaleChange={setFontScale}
                onTerminalFontSizeChange={setFontSize}
                onTerminalFontFamilyChange={setFontFamily}
                onTerminalScrollbackChange={setScrollback}
                onPreferredShellChange={setPreferredShellId}
                onBindKey={bind}
                onResetKeybindings={resetAll}
                onInstallUpdate={installUpdate}
                integrations={integrations.integrations}
                integrationActionError={integrations.actionError}
                onIntegrationInstall={(agent) =>
                  void integrations.install(agent)
                }
                onIntegrationUninstall={(agent) =>
                  void integrations.uninstall(agent)
                }
              />
            </main>
          </>
        </Suspense>
      )}
    </div>
  );
}

export default App;
