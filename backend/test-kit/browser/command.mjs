import { existsSync, readdirSync } from "node:fs";
import path from "node:path";
import { chromium } from "playwright";

const PLAYWRIGHT_HEADLESS_SWITCHES = [
  "--disable-field-trial-config",
  "--disable-background-networking",
  "--disable-background-timer-throttling",
  "--disable-backgrounding-occluded-windows",
  "--disable-back-forward-cache",
  "--disable-breakpad",
  "--disable-component-extensions-with-background-pages",
  "--disable-component-update",
  "--no-default-browser-check",
  "--disable-default-apps",
  "--disable-dev-shm-usage",
  "--disable-edgeupdater",
  "--disable-extensions",
  "--disable-features=AvoidUnnecessaryBeforeUnloadCheckSync,DestroyProfileOnBrowserClose,HttpsUpgrades,HttpsFirstBalancedModeAutoEnable,LensOverlay,MediaRouter,PaintHolding,ThirdPartyStoragePartitioning,AvoidCorsURLLoaderRestartOnRedirect,TcpPortRandomizationWin,OptimizationHints,NetworkTimeServiceQuerying,AimEnabled,PreconnectToSearch,msForceBrowserSignIn,msEdgeUpdateLaunchServicesPreferredVersion",
  "--enable-features=CDPScreenshotNewSurface,SkipListAccountsWithoutGaiaCookies",
  "--allow-pre-commit-input",
  "--disable-hang-monitor",
  "--disable-ipc-flooding-protection",
  "--disable-popup-blocking",
  "--disable-prompt-on-repost",
  "--disable-renderer-backgrounding",
  "--disable-updater-scheduler",
  "--force-color-profile=srgb",
  "--metrics-recording-only",
  "--no-first-run",
  "--password-store=basic",
  "--use-mock-keychain",
  "--no-service-autorun",
  "--disable-search-engine-choice-screen",
  "--unsafely-disable-devtools-self-xss-warnings",
  "--enable-unsafe-swiftshader",
  "--edge-skip-compat-layer-relaunch",
  "--disable-stack-profiler",
  "--disable-infobars",
  "--disable-sync",
  "--do-not-de-elevate",
  "--headless",
  "--hide-scrollbars",
  "--mute-audio",
  "--blink-settings=primaryHoverType=2,availableHoverTypes=2,primaryPointerType=4,availablePointerTypes=4",
  "--no-sandbox",
  "--no-startup-window",
];

export function pinnedShell() {
  const fullBrowserPath = chromium.executablePath();
  const cache = /^(?<cacheRoot>.*)[\\/]chromium-(?<revision>\d+)[\\/]/.exec(fullBrowserPath);
  if (!cache) throw new Error(`cannot tell Playwright's browser cache from ${fullBrowserPath}`);
  const { cacheRoot, revision } = cache.groups;
  const directory = path.join(cacheRoot, `chromium_headless_shell-${revision}`);
  const platform = existsSync(directory)
    ? readdirSync(directory).find((name) => name.startsWith("chrome-headless-shell-"))
    : undefined;
  const shell = platform ? path.join(directory, platform, "chrome-headless-shell") : undefined;
  if (!shell || !existsSync(shell)) {
    throw new Error(`the pinned Chromium is not installed under ${directory}: run sh scripts/prerequisites.sh`);
  }
  return shell;
}

if (import.meta.main) {
  const [dataDir, netLog] = process.argv.slice(2);
  if (!dataDir || !netLog) {
    console.error("usage: node test-kit/browser/command.mjs DATA_DIR NET_LOG");
    process.exit(64);
  }
  try {
    const command = [
      pinnedShell(),
      ...PLAYWRIGHT_HEADLESS_SWITCHES,
      "--no-system-proxy-config-service",
      `--user-data-dir=${dataDir}`,
      "--remote-debugging-port=0",
      `--log-net-log=${netLog}`,
    ];
    console.log(command.join("\n"));
  } catch (error) {
    console.error(error.message);
    process.exit(1);
  }
}
