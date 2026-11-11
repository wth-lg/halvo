use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use chromiumoxide::cdp::browser_protocol::fetch::{
    EnableParams as FetchEnableParams, EventRequestPaused, FulfillRequestParams, HeaderEntry,
    RequestPattern, RequestStage,
};
use chromiumoxide::cdp::browser_protocol::network::{Cookie, CookieParam, TimeSinceEpoch};
use chromiumoxide::cdp::browser_protocol::page::NavigateParams;
use chromiumoxide::cdp::browser_protocol::storage::{GetCookiesParams, SetCookiesParams};
use chromiumoxide::cdp::js_protocol::runtime::EvaluateParams;
use chromiumoxide::Page;
use futures::StreamExt;
use serde_json::{json, Value};

use super::{cdp, Executor, Fault, Res, POLL, STEP_TIMEOUT};
use crate::procs;

const STORAGE_PATH: &str = "/.halvo-storage";
const STORAGE_PAGE: &str = "<!doctype html><title>storage</title>";
const READ_STORAGE_JS: &str = "(() => { const out = []; for (let i = 0; i < localStorage.length; i++) { const k = localStorage.key(i); out.push([k, localStorage.getItem(k)]); } return out; })()";
const STOP_WAIT: Duration = Duration::from_secs(15);
const KILL_AFTER: Duration = Duration::from_secs(10);
const REMOVE_WAIT: Duration = Duration::from_secs(10);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionState {
    pub origins: Vec<String>,
    pub cookies: Vec<Cookie>,
    pub storage: Vec<OriginStorage>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OriginStorage {
    pub origin: String,
    pub items: Vec<(String, String)>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stopped {
    pub pids: Vec<u32>,
    pub adopted: Vec<u32>,
    pub left: Vec<u32>,
    pub profile: Option<PathBuf>,
    pub removed: bool,
}

fn state_fault(detail: impl std::fmt::Display) -> Fault {
    Fault::new("state", detail.to_string())
}

impl SessionState {
    pub fn to_json(&self) -> String {
        let storage: Vec<Value> = self
            .storage
            .iter()
            .map(|s| json!({"origin": s.origin, "items": s.items}))
            .collect();
        json!({"origins": self.origins, "cookies": self.cookies, "storage": storage}).to_string()
    }

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

    pub fn without_cookie(&self, name: &str) -> SessionState {
        let mut cut = self.clone();
        cut.cookies.retain(|c| c.name != name);
        cut
    }

    pub fn item_count(&self) -> usize {
        self.storage.iter().map(|s| s.items.len()).sum()
    }
}

pub fn host_of(origin: &str) -> &str {
    let rest = origin.split_once("://").map(|(_, r)| r).unwrap_or(origin);
    let end = rest.find([':', '/']).unwrap_or(rest.len());
    &rest[..end]
}

pub fn domain_matches(cookie_domain: &str, host: &str) -> bool {
    match cookie_domain.strip_prefix('.') {
        Some(d) => host == d || host.ends_with(&format!(".{d}")),
        None => cookie_domain == host,
    }
}

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

async fn evaluate(page: &Page, script: &str) -> Res<Value> {
    let params = EvaluateParams::builder()
        .expression(script)
        .return_by_value(true)
        .build()
        .map_err(|e| Fault::new("eval", e))?;
    let r = page.execute(params).await.map_err(cdp("eval"))?;
    if r.result.exception_details.is_some() {
        return Err(Fault::new("eval", "exception"));
    }
    Ok(r.result.result.value.unwrap_or(Value::Null))
}

async fn remove_profile(dir: &Path) -> bool {
    let deadline = Instant::now() + REMOVE_WAIT;
    loop {
        match std::fs::remove_dir_all(dir) {
            Ok(()) => return !dir.exists(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return true,
            Err(_) if Instant::now() < deadline => tokio::time::sleep(POLL).await,
            Err(_) => return !dir.exists(),
        }
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
    pub async fn export_state(&self, origins: &[String]) -> Res<SessionState> {
        let r = self
            .browser
            .execute(GetCookiesParams::default())
            .await
            .map_err(cdp("state"))?;
        let cookies: Vec<Cookie> = r
            .result
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

    pub async fn import_state(&self, state: &SessionState) -> Res<()> {
        let params: Vec<CookieParam> = state
            .cookies
            .iter()
            .map(|c| cookie_param(c, &state.origins))
            .collect();
        if !params.is_empty() {
            self.browser
                .execute(SetCookiesParams::new(params))
                .await
                .map_err(cdp("state"))?;
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

    pub(crate) async fn in_origin_page(&self, origin: &str, script: &str) -> Res<Value> {
        let page = self
            .browser
            .new_page("about:blank")
            .await
            .map_err(cdp("storage"))?;
        let outcome = in_page(&page, origin, script).await;
        let _ = page.close().await;
        outcome
    }

    pub async fn stop_and_remove(self) -> Stopped {
        let profile = self.profile.clone();
        let before = procs::table().unwrap_or_default();
        let root = self
            .pid
            .and_then(|pid| before.iter().find(|p| p.pid == pid).cloned());
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
        let stopped_at = Instant::now();
        self.stop().await;
        let left = loop {
            let now = procs::table().unwrap_or_default();
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
            if living.is_empty() || stopped_at.elapsed() > STOP_WAIT {
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
        let removed = match &profile {
            Some(dir) => remove_profile(dir).await,
            None => false,
        };
        Stopped {
            pids: pids(&tree),
            adopted: pids(&adopted),
            left: pids(&left),
            profile,
            removed,
        }
    }
}

async fn in_page(page: &Page, origin: &str, script: &str) -> Res<Value> {
    use base64::Engine;
    let mut events = page
        .event_listener::<EventRequestPaused>()
        .await
        .map_err(cdp("storage"))?;
    let answering = page.clone();
    let body = base64::engine::general_purpose::STANDARD.encode(STORAGE_PAGE);
    let task = tokio::spawn(async move {
        while let Some(ev) = events.next().await {
            let params = FulfillRequestParams::builder()
                .request_id(ev.request_id.clone())
                .response_code(200)
                .response_header(HeaderEntry::new("Content-Type", "text/html; charset=utf-8"))
                .body(body.clone())
                .build();
            if let Ok(p) = params {
                let _ = answering.execute(p).await;
            }
        }
    });
    let outcome = async {
        let pattern = RequestPattern::builder()
            .url_pattern(format!("{origin}/*"))
            .request_stage(RequestStage::Request)
            .build();
        page.execute(FetchEnableParams::builder().pattern(pattern).build())
            .await
            .map_err(cdp("storage"))?;
        let r = page
            .execute(NavigateParams::new(format!("{origin}{STORAGE_PATH}")))
            .await
            .map_err(cdp("storage"))?;
        if let Some(err) = r.result.error_text {
            return Err(Fault::new("storage", err));
        }
        let deadline = Instant::now() + STEP_TIMEOUT;
        loop {
            let here = evaluate(page, "location.origin + ' ' + document.readyState").await;
            if matches!(&here, Ok(Value::String(s)) if *s == format!("{origin} complete")) {
                break;
            }
            if Instant::now() > deadline {
                return Err(Fault::new("timeout", "storage_page"));
            }
            tokio::time::sleep(POLL).await;
        }
        evaluate(page, script).await
    }
    .await;
    task.abort();
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cdp::StartOptions;
    use crate::procs;

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

    fn browser() -> PathBuf {
        PathBuf::from(
            std::env::var("R3_TEST_BROWSER")
                .expect("R3_TEST_BROWSER names the browser binary for the ignored tests"),
        )
    }

    fn options(profile: PathBuf) -> StartOptions {
        let mut flags: Vec<String> = crate::FIRST_FLAGS.iter().map(|f| f.to_string()).collect();
        flags.push(crate::RESOLVER_RULE.to_string());
        StartOptions {
            browser: browser(),
            profile,
            headless: true,
            flags,
            debug_port: None,
        }
    }

    #[tokio::test]
    #[ignore = "starts the browser that R3_TEST_BROWSER names; run with --ignored"]
    async fn export_import_and_stop_carry_a_session_into_a_new_profile() {
        let scratch = std::env::temp_dir().join(format!("r3-web-state-{}", std::process::id()));
        std::fs::create_dir_all(&scratch).unwrap();
        let origin = "http://state.agent.test:18599".to_string();
        let a_dir = scratch.join("a");
        let a = Executor::start(&options(a_dir.clone())).await.unwrap();
        let set = a
            .in_origin_page(
                &origin,
                "(() => { document.cookie = 't1=v1; path=/'; localStorage.setItem('k', 'v'); return 1; })()",
            )
            .await
            .unwrap();
        assert_eq!(set, serde_json::json!(1));
        let exported = a.export_state(std::slice::from_ref(&origin)).await.unwrap();
        assert_eq!(
            exported
                .cookies
                .iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>(),
            vec!["t1"]
        );
        assert_eq!(
            exported.storage,
            vec![OriginStorage {
                origin: origin.clone(),
                items: vec![("k".into(), "v".into())]
            }]
        );
        let stopped = a.stop_and_remove().await;
        assert!(stopped.removed);
        assert!(!a_dir.exists());
        assert!(stopped.pids.len() >= 2, "{stopped:?}");
        assert!(stopped.left.is_empty(), "{stopped:?}");
        let table = procs::table().unwrap();
        assert!(
            table.iter().all(|p| !stopped.pids.contains(&p.pid)),
            "{stopped:?}"
        );
        let b_dir = scratch.join("b");
        let b = Executor::start(&options(b_dir.clone())).await.unwrap();
        b.import_state(&exported).await.unwrap();
        let again = b.export_state(std::slice::from_ref(&origin)).await.unwrap();
        assert_eq!(
            again
                .cookies
                .iter()
                .map(|c| (c.name.as_str(), c.value.as_str()))
                .collect::<Vec<_>>(),
            vec![("t1", "v1")]
        );
        assert_eq!(again.storage, exported.storage);
        let read = b
            .in_origin_page(&origin, "document.cookie + ' ' + localStorage.getItem('k')")
            .await
            .unwrap();
        assert_eq!(read, serde_json::json!("t1=v1 v"));
        let stopped = b.stop_and_remove().await;
        assert!(stopped.removed && !b_dir.exists(), "{stopped:?}");
    }
}
