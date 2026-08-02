import type { AppearancePreference } from "../appearance";
import {
  DEFAULT_FONT_SCALE,
  FONT_SCALE_STEP,
  formatFontScale,
  MAX_FONT_SCALE,
  MIN_FONT_SCALE,
} from "../fontScale";
import type { AppUpdateInfo, AppUpdateProgress } from "../host/types";
import type { AppUpdatePhase } from "../update/types";
import { Icon } from "../ui/Icon";

export type SettingsSection = "application" | "appearance";

const SETTINGS_SECTIONS: Array<{
  id: SettingsSection;
  label: string;
  description: string;
}> = [
  {
    id: "application",
    label: "Application",
    description: "Version and signed desktop updates",
  },
  {
    id: "appearance",
    label: "Appearance",
    description: "Theme and text size on this device",
  },
];

const APPEARANCE_OPTIONS: Array<{
  id: AppearancePreference;
  label: string;
  description: string;
  icon: "monitor" | "sun" | "moon";
}> = [
  {
    id: "system",
    label: "System",
    description: "Follow your device appearance",
    icon: "monitor",
  },
  {
    id: "light",
    label: "Light",
    description: "Use the light appearance",
    icon: "sun",
  },
  {
    id: "dark",
    label: "Dark",
    description: "Use the dark appearance",
    icon: "moon",
  },
];

export function SettingsSidebar({
  overlayTitlebar,
  section,
  onSectionChange,
  onBack,
}: {
  overlayTitlebar: boolean;
  section: SettingsSection;
  onSectionChange: (section: SettingsSection) => void;
  onBack: () => void;
}) {
  return (
    <aside className="sidebar settings-sidebar">
      <div
        className="window-nav settings-window-nav"
        {...(overlayTitlebar ? { "data-tauri-drag-region": "deep" } : {})}
      >
        <button className="settings-return" type="button" onClick={onBack}>
          <Icon name="arrow-right" size={15} />
          <span>Back to VINTAGE</span>
        </button>
      </div>

      <div className="settings-sidebar-heading">
        <h2>Settings</h2>
      </div>

      <nav className="settings-sidebar-nav" aria-label="Settings">
        {SETTINGS_SECTIONS.map((option) => (
          <button
            className={section === option.id ? "active" : ""}
            type="button"
            aria-current={section === option.id ? "page" : undefined}
            key={option.id}
            onClick={() => onSectionChange(option.id)}
          >
            <span className="settings-nav-label">{option.label}</span>
            <Icon name="arrow-right" size={12} />
          </button>
        ))}
      </nav>

      <div className="settings-sidebar-footer">
        <span className="avatar">V</span>
        <span>
          <strong>VINTAGE</strong>
          <small>Multi-agent terminal workspace</small>
        </span>
      </div>
    </aside>
  );
}

export function SettingsScreen({
  overlayTitlebar,
  section,
  appearance,
  fontScale,
  appVersion,
  update,
  updatePhase,
  updateProgress,
  updateNotice,
  updateError,
  onCheckForUpdates,
  onAppearanceChange,
  onFontScaleChange,
  onInstallUpdate,
}: {
  overlayTitlebar: boolean;
  section: SettingsSection;
  appearance: AppearancePreference;
  fontScale: number;
  appVersion: string | null;
  update: AppUpdateInfo | null;
  updatePhase: AppUpdatePhase;
  updateProgress: AppUpdateProgress | null;
  updateNotice: string | null;
  updateError: string | null;
  onCheckForUpdates: () => void;
  onAppearanceChange: (appearance: AppearancePreference) => void;
  onFontScaleChange: (scale: number) => void;
  onInstallUpdate: () => void;
}) {
  const checkingForUpdates = updatePhase === "checking";
  const updating = updatePhase === "downloading";
  const activeSection =
    SETTINGS_SECTIONS.find((option) => option.id === section) ??
    SETTINGS_SECTIONS[0];
  const updateButtonLabel = update
    ? updating
      ? "Updating…"
      : "Update & restart"
    : checkingForUpdates
      ? "Checking…"
      : "Check now";

  return (
    <div className="settings-page">
      <header
        className="taskbar settings-taskbar"
        {...(overlayTitlebar ? { "data-tauri-drag-region": "deep" } : {})}
      >
        <div className="taskbar-leading">
          <div className="task-title">
            <Icon name="sliders" />
            <strong>{activeSection.label}</strong>
          </div>
        </div>
      </header>

      <div className="settings-scroll">
        <div className="settings-content">
          <div className="settings-intro">
            <span>VINTAGE / SETTINGS</span>
            <h1>{activeSection.label}</h1>
            <p>{activeSection.description}.</p>
          </div>

          {section === "application" && (
            <section
              className="settings-card"
              aria-labelledby="application-settings-title"
            >
              <header>
                <div>
                  <h2 id="application-settings-title">Application</h2>
                  <p>Version and signed desktop updates.</p>
                </div>
              </header>
              <div className="settings-list">
                <div className="settings-row">
                  <div>
                    <strong>VINTAGE version</strong>
                    <small>The version installed on this device.</small>
                  </div>
                  <span className="settings-value">
                    {appVersion ? `Version ${appVersion}` : "Unavailable"}
                  </span>
                </div>
                <div className="settings-row settings-update-row">
                  <div>
                    <strong>Software updates</strong>
                    <small>
                      {update
                        ? `Version ${update.version} is available. VINTAGE found it automatically.`
                        : "VINTAGE checks automatically and will notify you when a new version is ready."}
                    </small>
                  </div>
                  <button
                    type="button"
                    disabled={checkingForUpdates || updating}
                    onClick={update ? onInstallUpdate : onCheckForUpdates}
                  >
                    <Icon name={update ? "download" : "refresh"} size={14} />
                    {updateButtonLabel}
                  </button>
                </div>
                {updateNotice && (
                  <p className="settings-inline-notice" role="status">
                    {updateNotice}
                  </p>
                )}
                {updateProgress && (
                  <div className="settings-row">
                    <div>
                      <strong>Update progress</strong>
                      <small>
                        {updateProgress.stage === "downloading"
                          ? "Downloading…"
                          : updateProgress.stage === "installing"
                            ? "Installing…"
                            : "Prepared."}
                      </small>
                    </div>
                    <span className="settings-value">
                      {updateProgress.downloaded > 0
                        ? `${Math.round(updateProgress.downloaded / 1024)} KiB`
                        : ""}
                    </span>
                  </div>
                )}
                {updateError && (
                  <p className="settings-inline-notice" role="alert">
                    {updateError}
                  </p>
                )}
              </div>
            </section>
          )}

          {section === "appearance" && (
            <section
              className="settings-card appearance-settings-card"
              aria-labelledby="appearance-settings-title"
            >
              <header>
                <div>
                  <h2 id="appearance-settings-title">Appearance</h2>
                  <p>Theme and text size for this device.</p>
                </div>
              </header>
              <fieldset className="appearance-options">
                <legend>Application theme</legend>
                <div className="appearance-option-grid">
                  {APPEARANCE_OPTIONS.map((option) => (
                    <label
                      className={`appearance-option ${appearance === option.id ? "selected" : ""}`}
                      key={option.id}
                    >
                      <input
                        type="radio"
                        name="appearance"
                        value={option.id}
                        checked={appearance === option.id}
                        onChange={() => onAppearanceChange(option.id)}
                      />
                      <span
                        className="appearance-preview"
                        data-appearance={option.id}
                        aria-hidden="true"
                      >
                        <span className="appearance-preview-sidebar" />
                        <span className="appearance-preview-content">
                          <i />
                          <i />
                          <i />
                        </span>
                      </span>
                      <span className="appearance-option-label">
                        <span>
                          <Icon name={option.icon} size={14} />
                          <strong>{option.label}</strong>
                        </span>
                        <small>{option.description}</small>
                      </span>
                      <span
                        className="appearance-option-check"
                        aria-hidden="true"
                      >
                        <Icon name="check" size={11} />
                      </span>
                    </label>
                  ))}
                </div>
              </fieldset>
              <div className="settings-list">
                <div className="settings-row">
                  <div>
                    <strong>Font size</strong>
                    <small>
                      Change text size across the app without resizing the
                      window layout.
                    </small>
                  </div>
                  <div
                    className="font-scale-control"
                    role="group"
                    aria-label="Font size"
                  >
                    <button
                      type="button"
                      aria-label="Decrease font size"
                      disabled={fontScale <= MIN_FONT_SCALE}
                      onClick={() =>
                        onFontScaleChange(fontScale - FONT_SCALE_STEP)
                      }
                    >
                      −
                    </button>
                    <span className="font-scale-value" aria-live="polite">
                      {formatFontScale(fontScale)}
                    </span>
                    <button
                      type="button"
                      aria-label="Increase font size"
                      disabled={fontScale >= MAX_FONT_SCALE}
                      onClick={() =>
                        onFontScaleChange(fontScale + FONT_SCALE_STEP)
                      }
                    >
                      +
                    </button>
                    <button
                      className="font-scale-reset"
                      type="button"
                      disabled={fontScale === DEFAULT_FONT_SCALE}
                      onClick={() => onFontScaleChange(DEFAULT_FONT_SCALE)}
                    >
                      Reset
                    </button>
                  </div>
                </div>
              </div>
            </section>
          )}
        </div>
      </div>
    </div>
  );
}
