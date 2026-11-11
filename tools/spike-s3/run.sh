#!/bin/sh
# Spike S3 on windows-2025: browser presence, the five password-field targets,
# and the cost of an attached UI Automation client. Prints counts, times,
# sizes, booleans and version numbers only.
set -u
exe=tools/spike-s3/target/release/spike-s3.exe
runs=${S3_COST_RUNS:-3}

for b in "chrome:C:/Program Files/Google/Chrome/Application/chrome.exe" \
  "edge:C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe" \
  "firefox:C:/Program Files/Mozilla Firefox/firefox.exe"; do
  name=${b%%:*}
  path=${b#*:}
  if [ -f "$path" ]; then
    version=$(powershell -NoProfile -Command "(Get-Item '$path').VersionInfo.FileVersion" | tr -d '\r')
    echo "S3 browser name=$name present=true version=$version"
  else
    echo "S3 browser name=$name present=false"
  fi
done

# First-run screens off, inside this throwaway VM only.
reg add 'HKLM\SOFTWARE\Policies\Microsoft\Edge' //v HideFirstRunExperience //t REG_DWORD //d 1 //f >/dev/null 2>&1
echo "S3 policy edge_hide_first_run=$?"
ff_dist='C:/Program Files/Mozilla Firefox/distribution'
if [ -d 'C:/Program Files/Mozilla Firefox' ]; then
  mkdir -p "$ff_dist"
  printf '%s\n' '{"policies":{"DontCheckDefaultBrowser":true,"DisableTelemetry":true,"OverrideFirstRunPage":"","OverridePostUpdatePage":"","UserMessaging":{"SkipOnboarding":true,"ExtensionRecommendations":false,"FeatureRecommendations":false,"MoreFromMozilla":false,"UrlbarInterventions":false}}}' >"$ff_dist/policies.json"
  echo "S3 policy firefox_policies_written=$?"
fi

timeout 60 "$exe" setup
for target in win32 wpf chrome edge firefox; do
  echo "S3 begin target=$target"
  timeout 600 "$exe" probe "$target"
  echo "S3 end target=$target exit=$?"
done

i=0
while [ "$i" -lt "$runs" ]; do
  i=$((i + 1))
  for browser in chrome edge firefox; do
    for condition in none uia; do
      timeout 300 "$exe" cost "$browser" "$condition" "$i"
      code=$?
      [ "$code" -eq 0 ] || echo "S3 cost browser=$browser condition=$condition run=$i exit=$code"
    done
  done
done
echo "S3 done"
