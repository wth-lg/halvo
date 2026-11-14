use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use chromiumoxide::cdp::browser_protocol::fetch::{
    EnableParams as FetchEnableParams, RequestPattern, RequestStage,
};
use chromiumoxide::cdp::browser_protocol::network::{Cookie, CookieParam, TimeSinceEpoch};
use chromiumoxide::cdp::browser_protocol::page::NavigateParams;
use chromiumoxide::cdp::browser_protocol::storage::{GetCookiesParams, SetCookiesParams};
use chromiumoxide::cdp::browser_protocol::target::{
    CloseTargetParams, CreateTargetParams, TargetId,
};
use chromiumoxide::cdp::js_protocol::runtime::EvaluateParams;
use serde_json::{json, Value};

use super::{Executor, Fault, Res, POLL, STEP_TIMEOUT};
use crate::procs;

const STORAGE_PATH: &str = "/.halvo-storage";
pub(crate) const STORAGE_PAGE: &str = "<!doctype html><title>storage</title>";
const READ_STORAGE_JS: &str = "(() => { const out = []; for (let i = 0; i < localStorage.length; i++) { const k = localStorage.key(i); out.push([k, localStorage.getItem(k)]); } return out; })()";
const STOP_WAIT: Duration = Duration::from_secs(15);
const KILL_AFTER: Duration = Duration::from_secs(10);
const REMOVE_WAIT: Duration = Duration::from_secs(10);

/// The state that carries a sign-in from one browser into the next: the origins it was exported
/// for, the browser's cookies whose domain belongs to one of their hosts, and the localStorage items
/// of each origin. It holds credentials.
#[derive(Clone, Default, PartialEq)]
pub struct SessionState {
    pub origins: Vec<String>,
    pub cookies: Vec<Cookie>,
    pub storage: Vec<OriginStorage>,
}

/// Writes each field's name with its count and no value, so a logged state holds no credential.
impl std::fmt::Debug for SessionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionState")
            .field("origins", &self.origins.len())
            .field("cookies", &self.cookies.len())
            .field("storage", &self.storage.len())
            .finish()
    }
}

/// The localStorage items of one origin, sorted by key.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct OriginStorage {
    pub origin: String,
    pub items: Vec<(String, String)>,
}

/// Writes the count of items and no origin, key or stored value, so a logged state holds none.
impl std::fmt::Debug for OriginStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OriginStorage")
            .field("items", &self.items.len())
            .finish_non_exhaustive()
    }
}

/// What `stop_and_remove` did: the pids of the browser's process tree (`pids`); the processes that
/// left the tree and were waited for but never ended (`adopted`, on macOS the crash handler); the
/// pids of either kind that still ran when the wait ended (`left`); the profile directory and the
/// temporary directory that the start made; and whether both are gone (`removed`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stopped {
    pub pids: Vec<u32>,
    pub adopted: Vec<u32>,
    pub left: Vec<u32>,
    /// What the end of the run's own processes found before it ended anything and what its last
    /// pass found (CP-126 E1 (d)), over the stop's end and the one after its wait.
    pub ended: crate::checks::Ended,
    pub profile: Option<PathBuf>,
    pub temp: Option<PathBuf>,
    pub removed: bool,
    /// Whether the last read of the stop's wait found the process table unread, so that the wait
    /// stayed open until `STOP_WAIT` and `left` may lack a process (CP-170 E15).
    pub table_unread: bool,
}

/// Whether the stop's wait ends at a read: when nothing of the run runs and the table was read, or
/// when `STOP_WAIT` has passed; a read of an unread table is never one that found nothing.
fn wait_ends(nothing_runs: bool, unread: bool, timed_out: bool) -> bool {
    (nothing_runs && !unread) || timed_out
}

fn state_fault(detail: impl std::fmt::Display) -> Fault {
    Fault::new("state", detail.to_string())
}

impl SessionState {
    /// The JSON form `{"origins":[..],"cookies":[..],"storage":[{"origin":..,"items":[[k,v],..]}]}`.
    pub fn to_json(&self) -> String {
        let storage: Vec<Value> = self
            .storage
            .iter()
            .map(|s| json!({"origin": s.origin, "items": s.items}))
            .collect();
        json!({"origins": self.origins, "cookies": self.cookies, "storage": storage}).to_string()
    }

    /// Reads the form of `to_json` back; any other text is a fault `state`.
    pub fn from_json(text: &str) -> Res<SessionState> {
        let v: Value = serde_json::from_str(text).map_err(state_fault)?;
        let origins: Vec<String> =
            serde_json::from_value(v["origins"].clone()).map_err(state_fault)?;
        let cookies: Vec<Cookie> =
            serde_json::from_value(v["cookies"].clone()).map_err(state_fault)?;
        let mut storage = Vec::new();
        for s in v["storage"]
            .as_array()
            .ok_or_else(|| state_fault("storage"))?
        {
            let origin = s["origin"]
                .as_str()
                .ok_or_else(|| state_fault("origin"))?
                .to_string();
            let items: Vec<(String, String)> =
                serde_json::from_value(s["items"].clone()).map_err(state_fault)?;
            storage.push(OriginStorage { origin, items });
        }
        Ok(SessionState {
            origins,
            cookies,
            storage,
        })
    }

    /// A copy without the cookies of that name.
    pub fn without_cookie(&self, name: &str) -> SessionState {
        let mut cut = self.clone();
        cut.cookies.retain(|c| c.name != name);
        cut
    }

    /// The number of localStorage items over all origins.
    pub fn item_count(&self) -> usize {
        self.storage.iter().map(|s| s.items.len()).sum()
    }
}

/// The host of an origin, without its scheme and port.
pub fn host_of(origin: &str) -> &str {
    let rest = origin.split_once("://").map(|(_, r)| r).unwrap_or(origin);
    let end = rest.find([':', '/']).unwrap_or(rest.len());
    &rest[..end]
}

/// Whether a cookie of `cookie_domain` belongs to `host`: the same host for a host-only cookie, the
/// domain or a subdomain of it for a domain cookie (a leading dot).
pub fn domain_matches(cookie_domain: &str, host: &str) -> bool {
    match cookie_domain.strip_prefix('.') {
        Some(d) => host == d || host.ends_with(&format!(".{d}")),
        None => cookie_domain == host,
    }
}

/// The parameters that set `cookie` again in another browser: through the origin's address for a
/// host-only cookie of one of `origins`, else through its domain; with its path, secure, httpOnly,
/// sameSite, priority and partition key, and with its expiry unless it is a session cookie.
pub fn cookie_param(cookie: &Cookie, origins: &[String]) -> CookieParam {
    let mut p = CookieParam::new(cookie.name.clone(), cookie.value.clone());
    let own = origins.iter().find(|o| host_of(o) == cookie.domain);
    match own {
        Some(o) if !cookie.domain.starts_with('.') => p.url = Some(format!("{o}/")),
        _ => p.domain = Some(cookie.domain.clone()),
    }
    p.path = Some(cookie.path.clone());
    p.secure = Some(cookie.secure);
    p.http_only = Some(cookie.http_only);
    p.same_site = cookie.same_site.clone();
    p.expires = (!cookie.session).then(|| TimeSinceEpoch::new(cookie.expires));
    p.priority = Some(cookie.priority.clone());
    p.partition_key = cookie.partition_key.clone();
    p
}

async fn evaluate(ex: &Executor, session: &str, script: &str) -> Res<Value> {
    let params = EvaluateParams::builder()
        .expression(script)
        .return_by_value(true)
        .build()
        .map_err(|e| Fault::new("eval", e))?;
    let r = ex
        .wire
        .call(Some(session), params)
        .await
        .map_err(|f| Fault::new("eval", f.1))?;
    if r.exception_details.is_some() {
        return Err(Fault::new("eval", "exception"));
    }
    Ok(r.result.value.unwrap_or(Value::Null))
}

pub(crate) fn temp_of(profile: &Path) -> Option<PathBuf> {
    let name = profile.file_name()?;
    let mut temp = name.to_owned();
    temp.push("-tmp");
    Some(profile.parent()?.join(temp))
}

pub(crate) fn make_dirs(profile: &Path) -> Res<(PathBuf, PathBuf)> {
    let temp = temp_of(profile).ok_or_else(|| Fault::new("start", "profile path has no name"))?;
    for (dir, what) in [(profile, "profile"), (temp.as_path(), "temporary")] {
        match std::fs::symlink_metadata(dir) {
            Ok(m) if m.file_type().is_symlink() => {
                return Err(Fault::new("start", format!("{what} path is a link")))
            }
            Ok(_) => return Err(Fault::new("start", format!("{what} path exists"))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(Fault::new("start", e.to_string())),
        }
    }
    std::fs::create_dir(profile).map_err(|e| Fault::new("start", e.to_string()))?;
    if let Err(e) = std::fs::create_dir(&temp) {
        let _ = std::fs::remove_dir(profile);
        return Err(Fault::new("start", e.to_string()));
    }
    match (std::fs::canonicalize(profile), std::fs::canonicalize(&temp)) {
        (Ok(p), Ok(t)) => Ok((p, t)),
        _ => {
            let _ = std::fs::remove_dir(profile);
            let _ = std::fs::remove_dir(&temp);
            Err(Fault::new("start", "canonical path"))
        }
    }
}

fn gone(dir: &Path) -> bool {
    matches!(std::fs::symlink_metadata(dir), Err(e) if e.kind() == std::io::ErrorKind::NotFound)
}

async fn remove_profile(dir: &Path) -> bool {
    let deadline = Instant::now() + REMOVE_WAIT;
    loop {
        match std::fs::symlink_metadata(dir) {
            Ok(m) if m.is_dir() => {}
            Ok(_) => return false,
            Err(_) => return gone(dir),
        }
        match std::fs::remove_dir_all(dir) {
            Ok(()) => return gone(dir),
            Err(_) if Instant::now() < deadline => tokio::time::sleep(POLL).await,
            Err(_) => return gone(dir),
        }
    }
}

pub(crate) async fn remove_dirs(profile: &Path, temp: &Path) -> bool {
    let profile_gone = remove_profile(profile).await;
    let temp_gone = remove_profile(temp).await;
    profile_gone && temp_gone
}

pub(crate) struct RemoveOnDrop(Option<(PathBuf, PathBuf)>);

impl RemoveOnDrop {
    pub(crate) fn new(profile: &Path, temp: &Path) -> RemoveOnDrop {
        RemoveOnDrop(Some((profile.to_path_buf(), temp.to_path_buf())))
    }

    pub(crate) fn disarm(mut self) {
        self.0 = None;
    }
}

impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        if let Some((profile, temp)) = self.0.take() {
            remove_now(&profile);
            remove_now(&temp);
        }
    }
}

fn remove_now(dir: &Path) {
    let deadline = Instant::now() + REMOVE_WAIT;
    let mut quiet = 0;
    while quiet < 6 && Instant::now() < deadline {
        match std::fs::symlink_metadata(dir) {
            Ok(m) if m.is_dir() => {
                quiet = 0;
                let _ = std::fs::remove_dir_all(dir);
            }
            Ok(_) => return,
            Err(_) => quiet += 1,
        }
        std::thread::sleep(POLL);
    }
}

fn push_new(into: &mut Vec<procs::Proc>, from: Vec<procs::Proc>) {
    for p in from {
        if !into.iter().any(|q| procs::same(q, &p)) {
            into.push(p);
        }
    }
}

fn pids(list: &[procs::Proc]) -> Vec<u32> {
    let mut v: Vec<u32> = list.iter().map(|p| p.pid).collect();
    v.sort_unstable();
    v
}

impl Executor {
    /// Reads the browser's cookies that belong to the hosts of `origins` and the localStorage of each
    /// origin; the storage is read in a page of the origin that the browser answers itself, so no
    /// request reaches the site.
    pub async fn export_state(&self, origins: &[String]) -> Res<SessionState> {
        let r = self
            .wire
            .call(None, GetCookiesParams::default())
            .await
            .map_err(|f| Fault::new("state", f.1))?;
        let cookies: Vec<Cookie> = r
            .cookies
            .into_iter()
            .filter(|c| {
                origins
                    .iter()
                    .any(|o| domain_matches(&c.domain, host_of(o)))
            })
            .collect();
        let mut storage = Vec::new();
        for origin in origins {
            let read = self.in_origin_page(origin, READ_STORAGE_JS).await?;
            let mut items: Vec<(String, String)> =
                serde_json::from_value(read).map_err(state_fault)?;
            items.sort();
            if !items.is_empty() {
                storage.push(OriginStorage {
                    origin: origin.clone(),
                    items,
                });
            }
        }
        Ok(SessionState {
            origins: origins.to_vec(),
            cookies,
            storage,
        })
    }

    /// Sets the cookies of `state` and replaces the localStorage of each of its origins with its
    /// items; a fault `state` when an origin does not hold them afterwards.
    pub async fn import_state(&self, state: &SessionState) -> Res<()> {
        let params: Vec<CookieParam> = state
            .cookies
            .iter()
            .map(|c| cookie_param(c, &state.origins))
            .collect();
        if !params.is_empty() {
            self.wire
                .call(None, SetCookiesParams::new(params))
                .await
                .map_err(|f| Fault::new("state", f.1))?;
        }
        for s in &state.storage {
            let items = serde_json::to_string(&s.items).map_err(state_fault)?;
            let script = format!(
                "(() => {{ const items = {items}; localStorage.clear(); for (const [k, v] of items) localStorage.setItem(k, v); return localStorage.length; }})()"
            );
            let n = self.in_origin_page(&s.origin, &script).await?;
            if n.as_u64() != Some(s.items.len() as u64) {
                return Err(state_fault("storage_items"));
            }
        }
        Ok(())
    }

    /// Runs `script` in a new page of `origin` whose one document the executor answers itself, so
    /// no request reaches the site, and closes the page.
    pub(crate) async fn in_origin_page(&self, origin: &str, script: &str) -> Res<Value> {
        let made = self
            .wire
            .call(None, CreateTargetParams::new("about:blank"))
            .await
            .map_err(|f| Fault::new("storage", f.1))?;
        let target = made.target_id.inner().clone();
        let url = format!("{origin}{STORAGE_PATH}");
        self.wire.serve_storage_page(&url, true);
        let outcome = self.in_page(&target, origin, &url, script).await;
        self.wire.serve_storage_page(&url, false);
        let _ = self
            .wire
            .call(None, CloseTargetParams::new(TargetId::from(target)))
            .await;
        outcome
    }

    async fn in_page(&self, target: &str, origin: &str, url: &str, script: &str) -> Res<Value> {
        let deadline = Instant::now() + STEP_TIMEOUT;
        let session = loop {
            if let Some((s, _)) = self.wire.session_of(target) {
                break s;
            }
            if Instant::now() > deadline {
                return Err(Fault::new("storage", "page_session"));
            }
            tokio::time::sleep(POLL).await;
        };
        if self
            .wire
            .book
            .lock()
            .map(|b| b.rule.is_none())
            .unwrap_or(true)
        {
            let pattern = RequestPattern::builder()
                .url_pattern(format!("{origin}/*"))
                .request_stage(RequestStage::Request)
                .build();
            self.wire
                .call(
                    Some(&session),
                    FetchEnableParams::builder().pattern(pattern).build(),
                )
                .await
                .map_err(|f| Fault::new("storage", f.1))?;
        }
        let r = self
            .wire
            .call(Some(&session), NavigateParams::new(url.to_string()))
            .await
            .map_err(|f| Fault::new("storage", f.1))?;
        if let Some(err) = r.error_text {
            return Err(Fault::new("storage", err));
        }
        loop {
            let here = evaluate(
                self,
                &session,
                "location.origin + ' ' + document.readyState",
            )
            .await;
            if matches!(&here, Ok(Value::String(s)) if *s == format!("{origin} complete")) {
                break;
            }
            if Instant::now() > deadline {
                return Err(Fault::new("timeout", "storage_page"));
            }
            tokio::time::sleep(POLL).await;
        }
        evaluate(self, &session, script).await
    }

    /// Stops the browser and removes the two directories that its start made, the profile and the
    /// temporary directory beside it, and nothing else. It closes the browser (`stop`), then waits up
    /// to 15 s for every process of the tree and for the adopted ones; from 10 s on it ends each
    /// process of the tree that still has its recorded creation time, never an adopted one. Then it
    /// removes both directories, following no link, and reports `removed` false when anything of
    /// them is left.
    pub async fn stop_and_remove(self) -> Stopped {
        let profile = self.profile.clone();
        let temp = self.temp.clone();
        let before = procs::table().unwrap_or_default();
        let root = before.iter().find(|p| procs::same(p, &self.root)).cloned();
        let mut tree: Vec<procs::Proc> = root
            .iter()
            .flat_map(|r| procs::descendants(&before, std::slice::from_ref(r)))
            .collect();
        let since = root.as_ref().map(|p| p.started_us).unwrap_or(u64::MAX);
        let roots: Vec<PathBuf> = root
            .as_ref()
            .and_then(|p| p.exe.as_deref().map(procs::bundle_root))
            .into_iter()
            .collect();
        let mut adopted = procs::adopted(&before, &tree, since, &roots);
        let mark = self.mark.clone();
        let stopped_at = Instant::now();
        let (_, first) = self.stop().await;
        let mut unread;
        let left = loop {
            let read = procs::table();
            unread = read.is_err();
            let now = read.unwrap_or_default();
            let grown = procs::descendants(&now, &tree);
            push_new(&mut tree, grown);
            let more = procs::adopted(&now, &tree, since, &roots);
            push_new(&mut adopted, more);
            let living: Vec<procs::Proc> = tree
                .iter()
                .chain(adopted.iter())
                .filter(|p| procs::running(p))
                .cloned()
                .collect();
            if wait_ends(living.is_empty(), unread, stopped_at.elapsed() > STOP_WAIT) {
                break living;
            }
            if stopped_at.elapsed() > KILL_AFTER {
                for p in living
                    .iter()
                    .filter(|p| tree.iter().any(|t| procs::same(t, p)))
                {
                    procs::kill(p);
                }
            }
            tokio::time::sleep(POLL).await;
        };
        // a process that the run left while the stop waited is ended too, before the dirs go
        let ended = first.then(crate::checks::end_runs_own(&mark));
        let removed = match (&profile, &temp) {
            (Some(p), Some(t)) => remove_dirs(p, t).await,
            _ => false,
        };
        Stopped {
            pids: pids(&tree),
            adopted: pids(&adopted),
            left: pids(&left),
            ended,
            profile,
            temp,
            removed,
            table_unread: unread,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cdp::StartOptions;

    fn cookie(name: &str, domain: &str, http_only: bool, session: bool) -> Cookie {
        serde_json::from_value(serde_json::json!({
            "name": name,
            "value": format!("value-of-{name}"),
            "domain": domain,
            "path": "/",
            "expires": if session { -1.0 } else { 2_000_000_000.0 },
            "size": 10,
            "httpOnly": http_only,
            "secure": false,
            "session": session,
            "sameSite": "Lax",
            "priority": "Medium",
            "sourceScheme": "NonSecure",
            "sourcePort": 18599
        }))
        .unwrap()
    }

    fn state() -> SessionState {
        SessionState {
            origins: vec![
                "http://utility.agent.test:18599".into(),
                "http://pay.agent.test:18599".into(),
            ],
            cookies: vec![
                cookie("a", "utility.agent.test", true, true),
                cookie("b", "utility.agent.test", false, false),
                cookie("c", ".agent.test", true, true),
            ],
            storage: vec![OriginStorage {
                origin: "http://utility.agent.test:18599".into(),
                items: vec![
                    ("k".into(), "v \"quoted\"".into()),
                    ("k2".into(), String::new()),
                ],
            }],
        }
    }

    /// CP-170 E15 (review N3): a read of an unread table keeps the stop's wait open.
    #[test]
    fn an_unread_table_keeps_the_stop_s_wait_open() {
        assert!(!wait_ends(true, true, false));
        assert!(wait_ends(true, false, false));
        assert!(wait_ends(true, true, true));
        assert!(!wait_ends(false, false, false));
    }

    #[test]
    fn host_of_drops_scheme_and_port() {
        assert_eq!(
            host_of("http://utility.agent.test:18599"),
            "utility.agent.test"
        );
        assert_eq!(host_of("https://pay.agent.test"), "pay.agent.test");
    }

    #[test]
    fn domain_matches_host_only_and_domain_cookies() {
        assert!(domain_matches("utility.agent.test", "utility.agent.test"));
        assert!(!domain_matches("utility.agent.test", "pay.agent.test"));
        assert!(!domain_matches("agent.test", "utility.agent.test"));
        assert!(domain_matches(".agent.test", "utility.agent.test"));
        assert!(domain_matches(".utility.agent.test", "utility.agent.test"));
        assert!(!domain_matches(".gent.test", "utility.agent.test"));
        assert!(!domain_matches(".other.test", "utility.agent.test"));
    }

    #[test]
    fn a_host_only_cookie_keeps_its_host_and_port_through_the_url() {
        let s = state();
        let p = cookie_param(&s.cookies[0], &s.origins);
        assert_eq!(p.url.as_deref(), Some("http://utility.agent.test:18599/"));
        assert_eq!(p.domain, None);
        assert_eq!(p.path.as_deref(), Some("/"));
        assert_eq!(p.http_only, Some(true));
        assert_eq!(p.expires, None);
        assert_eq!(p.value, "value-of-a");
        let persistent = cookie_param(&s.cookies[1], &s.origins);
        assert!(persistent.expires.is_some());
        assert_eq!(persistent.http_only, Some(false));
        let domain = cookie_param(&s.cookies[2], &s.origins);
        assert_eq!(domain.domain.as_deref(), Some(".agent.test"));
        assert_eq!(domain.url, None);
    }

    #[test]
    fn state_round_trips_through_json_and_leaves_out_one_cookie() {
        let s = state();
        let text = s.to_json();
        assert!(!text.is_empty());
        assert_eq!(SessionState::from_json(&text).unwrap(), s);
        let cut = s.without_cookie("b");
        assert_eq!(
            cut.cookies
                .iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "c"]
        );
        assert_eq!(s.item_count(), 2);
        assert!(SessionState::from_json("{").is_err());
    }

    #[test]
    fn the_debug_text_of_a_state_holds_no_cookie_value() {
        let s = state();
        let text = format!("{s:?}");
        assert_eq!(s.cookies[0].value, "value-of-a");
        assert!(s.cookies.iter().all(|c| !text.contains(&c.value)), "{text}");
        assert_eq!(text, "SessionState { origins: 2, cookies: 3, storage: 1 }");
        let stored = format!("{:?} {:?}", s.storage, s.storage[0]);
        assert_eq!(s.storage[0].items[0].1, "v \"quoted\"");
        assert!(!stored.contains("quoted"), "{stored}");
        assert_eq!(
            stored,
            "[OriginStorage { items: 2, .. }] OriginStorage { items: 2, .. }"
        );
    }

    fn scratch(name: &str) -> PathBuf {
        crate::child::scratch(name)
    }

    async fn missing_browser(profile: PathBuf) -> StartOptions {
        let relay = crate::relay::Relay::start(crate::relay::Names::Fixture)
            .await
            .unwrap();
        let handle = relay.handle();
        relay.stop();
        crate::cdp::tests::options_with(crate::cdp::tests::nowhere("chrome"), profile, handle)
    }

    fn cause(f: Option<crate::cdp::StartFailure>) -> Option<(&'static str, String)> {
        f.map(|f| (f.cause, f.detail))
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn start_refuses_a_profile_path_that_is_a_link() {
        let dir = scratch("link-start");
        let target = dir.join("target");
        std::fs::create_dir_all(&target).unwrap();
        let link = dir.join("profile");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let refused = Executor::start(&missing_browser(link).await).await.err();
        assert_eq!(
            cause(refused),
            Some(("start", "profile path is a link".into()))
        );
        assert_eq!(std::fs::read_dir(&target).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn start_refuses_a_profile_path_that_exists() {
        let dir = scratch("exists-start");
        let profile = dir.join("profile");
        std::fs::create_dir_all(&profile).unwrap();
        let refused = Executor::start(&missing_browser(profile.clone()).await)
            .await
            .err();
        assert_eq!(
            cause(refused),
            Some(("start", "profile path exists".into()))
        );
        assert_eq!(std::fs::read_dir(&profile).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn start_refuses_a_temporary_path_that_exists_and_makes_no_profile() {
        let dir = scratch("temp-exists");
        let profile = dir.join("profile");
        let temp = dir.join("profile-tmp");
        std::fs::create_dir_all(&temp).unwrap();
        let refused = Executor::start(&missing_browser(profile.clone()).await)
            .await
            .err();
        assert_eq!(
            cause(refused),
            Some(("start", "temporary path exists".into()))
        );
        assert!(std::fs::symlink_metadata(&profile).is_err());
    }

    #[tokio::test]
    async fn a_dropped_start_removes_both_directories_that_it_made() {
        let dir = scratch("dropped-start");
        let (profile, temp) = make_dirs(&dir.join("profile")).unwrap();
        std::fs::write(profile.join("Local State"), b"{}").unwrap();
        std::fs::write(dir.join("beside.txt"), b"caller").unwrap();
        drop(RemoveOnDrop::new(&profile, &temp));
        assert!(gone(&profile) && gone(&temp));
        assert!(dir.join("beside.txt").exists());
        let (kept, kept_temp) = make_dirs(&dir.join("kept")).unwrap();
        RemoveOnDrop::new(&kept, &kept_temp).disarm();
        assert!(kept.is_dir() && kept_temp.is_dir());
    }

    #[test]
    fn the_temporary_directory_is_beside_a_profile_path_that_ends_in_a_separator() {
        let dir = scratch("trailing");
        let mut with_slash = dir.join("profile").into_os_string();
        with_slash.push("/");
        let (profile, temp) = make_dirs(Path::new(&with_slash)).unwrap();
        let real = dir.canonicalize().unwrap();
        assert_eq!(profile, real.join("profile"));
        assert_eq!(temp, real.join("profile-tmp"));
        assert!(make_dirs(Path::new("/")).is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn the_removal_takes_the_directory_that_start_made_when_a_link_above_it_moves() {
        let dir = scratch("moved-link");
        std::fs::create_dir_all(dir.join("a")).unwrap();
        std::fs::create_dir_all(dir.join("b").join("profile")).unwrap();
        std::fs::write(dir.join("b").join("profile").join("caller"), b"x").unwrap();
        std::os::unix::fs::symlink(dir.join("a"), dir.join("via")).unwrap();
        let (profile, temp) = make_dirs(&dir.join("via").join("profile")).unwrap();
        std::os::unix::fs::symlink(dir.join("b"), dir.join("via2")).unwrap();
        std::fs::rename(dir.join("via2"), dir.join("via")).unwrap();
        assert!(remove_dirs(&profile, &temp).await);
        assert!(!dir.join("a").join("profile").exists());
        assert!(dir.join("b").join("profile").join("caller").exists());
    }

    #[tokio::test]
    async fn a_failed_start_removes_both_directories_that_it_made() {
        let dir = scratch("failed-start");
        let profile = dir.join("profile");
        std::fs::write(dir.join("beside.txt"), b"caller").unwrap();
        let failed = Executor::start(&missing_browser(profile.clone()).await)
            .await
            .err();
        let failed = failed.expect("failed");
        assert_eq!((failed.cause, failed.removed), ("browser_start", true));
        assert!(std::fs::symlink_metadata(&profile).is_err());
        assert!(std::fs::symlink_metadata(dir.join("profile.stderr")).is_err());
        assert!(std::fs::symlink_metadata(dir.join("profile-tmp")).is_err());
        assert!(dir.join("beside.txt").exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_profile_path_turned_into_a_link_is_not_followed_and_is_not_removed() {
        let dir = scratch("link-stop");
        let profile = dir.join("profile");
        let target = dir.join("target");
        std::fs::create_dir_all(target.join("Default")).unwrap();
        std::fs::write(target.join("Default").join("Cookies"), b"kept").unwrap();
        std::os::unix::fs::symlink(&target, &profile).unwrap();
        assert!(!remove_profile(&profile).await);
        assert!(target.join("Default").join("Cookies").exists());
    }

    #[tokio::test]
    async fn the_removal_takes_the_profile_and_leaves_a_file_beside_it() {
        let dir = scratch("beside");
        let (profile, _) = make_dirs(&dir.join("profile")).unwrap();
        std::fs::create_dir_all(profile.join("Default")).unwrap();
        std::fs::write(profile.join("Default").join("Cookies"), b"session").unwrap();
        std::fs::write(dir.join("beside.txt"), b"caller").unwrap();
        assert!(remove_profile(&profile).await);
        assert!(gone(&profile));
        assert_eq!(std::fs::read(dir.join("beside.txt")).unwrap(), b"caller");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn the_removal_reports_no_when_a_file_of_the_profile_cannot_be_removed() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("stuck");
        let (profile, _) = make_dirs(&dir.join("profile")).unwrap();
        let locked = profile.join("locked");
        std::fs::create_dir_all(&locked).unwrap();
        std::fs::write(locked.join("file"), b"x").unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o500)).unwrap();
        let removed = remove_profile(&profile).await;
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(!removed);
        assert!(locked.join("file").exists());
    }

    /// CP-189 G3, a ban: the stop's code in this file holds no call that ends a process but the end
    /// of the run's own processes by the run's mark (`end_runs_own`). The first process is ended
    /// through the child's handle in cdp.rs. The text before this file's tests is read.
    #[test]
    fn g3_the_stop_holds_no_call_that_ends_a_process_without_the_mark() {
        let source = include_str!("session.rs");
        let code = source
            .split("#[cfg(test)]\nmod tests {")
            .next()
            .unwrap_or(source);
        for banned in [
            "procs::kill(",
            "procs::ask_end(",
            "libc::kill(",
            "TerminateProcess(",
            "end_tree_now(",
            ".kill()",
            "kill_tree(",
        ] {
            assert!(
                !code.contains(banned),
                "cdp/session.rs calls {banned}, which ends a process without the run's mark"
            );
        }
    }
}
