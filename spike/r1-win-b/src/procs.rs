// S7: the hidden app's summed working set with its web-view processes, and whether the page
// loads. S9: the bounded parent-pid walk. S8 (site cut): the peak working set of a command.
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use crate::util::*;
use windows::Win32::Foundation::*;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::WindowsAndMessaging::*;

#[derive(Default)]
struct Tally {
    max_sum_ws: u64,
    max_sum_private: u64,
    peak_by_pid: HashMap<u32, u64>,
    max_procs: usize,
    max_webview: usize,
    unreadable: usize,
    samples: usize,
    app_peak: u64,
}

fn sample_tree(root: u32, t: &mut Tally) -> (usize, u64) {
    let procs = snapshot();
    let tree = descendants(root, &procs);
    let names: HashMap<u32, &str> = procs.iter().map(|p| (p.pid, p.exe.as_str())).collect();
    let mut sum = 0u64;
    let mut sum_priv = 0u64;
    let mut wv = 0usize;
    for &pid in &tree {
        if names.get(&pid).map(|n| n.eq_ignore_ascii_case("msedgewebview2.exe")).unwrap_or(false) {
            wv += 1;
        }
        match open_query(pid).and_then(|h| memory(h.0)) {
            Some((ws, peak, pv)) => {
                sum += ws;
                sum_priv += pv;
                let e = t.peak_by_pid.entry(pid).or_insert(0);
                *e = (*e).max(peak);
                if pid == root {
                    t.app_peak = t.app_peak.max(peak);
                }
            }
            None => t.unreadable += 1,
        }
    }
    t.max_sum_ws = t.max_sum_ws.max(sum);
    t.max_sum_private = t.max_sum_private.max(sum_priv);
    t.max_procs = t.max_procs.max(tree.len());
    t.max_webview = t.max_webview.max(wv);
    t.samples += 1;
    (tree.len(), sum)
}

fn app_window_with_title(pid: u32, title: &str) -> (Option<HWND>, bool) {
    let mut first = None;
    for w in windows_all(false) {
        if w.pid == pid && w.top && w.class.starts_with("Tauri") || (w.pid == pid && w.top && first.is_none()) {
            let h = hwnd(w.hwnd);
            if window_text(h) == title {
                return (Some(h), true);
            }
            if first.is_none() && !window_text(h).is_empty() {
                first = Some(h);
            }
        }
    }
    (first, false)
}

/// Start the app `runs` times; each run samples its process tree every 250 ms until `idle`
/// seconds after the page-load mark (the title the app sets on PageLoadEvent::Finished), or
/// until `idle` seconds after a 90 s wait when no load is seen.
pub fn app_run(args: &[String]) {
    let exe = arg(args, "--exe").unwrap();
    let runs: usize = arg(args, "--runs").and_then(|s| s.parse().ok()).unwrap_or(5);
    let idle: u64 = arg(args, "--idle-secs").and_then(|s| s.parse().ok()).unwrap_or(60);
    let title = arg(args, "--title").unwrap_or_else(|| "Halvo 0.0.0".into());
    let label = arg(args, "--label").unwrap_or_else(|| "s7".into());
    let do_walk = flag(args, "--walk");
    let extra = arg(args, "--args").unwrap_or_default();
    for run in 1..=runs {
        let cmd = if extra.is_empty() { format!("\"{exe}\"") } else { format!("\"{exe}\" {extra}") };
        let Ok((h, pid)) = spawn(&cmd, PROCESS_CREATION_FLAGS(0)) else {
            println!("S7 app_run label={label} run={run} started=false");
            continue;
        };
        let t0 = Instant::now();
        let mut t = Tally::default();
        let mut loaded_at: Option<Duration> = None;
        let mut window_seen = false;
        let mut window_visible = false;
        let mut main_visible = false;
        let mut visible_kinds: Vec<String> = Vec::new();
        loop {
            sample_tree(pid, &mut t);
            if loaded_at.is_none() {
                let (w, ok) = app_window_with_title(pid, &title);
                if w.is_some() || ok {
                    window_seen = true;
                }
                if ok {
                    loaded_at = Some(t0.elapsed());
                }
            }
            for w in windows_all(false) {
                if w.pid == pid && w.top && w.visible {
                    window_visible = true;
                    let h = hwnd(w.hwnd);
                    let t = window_text(h);
                    if t == title || t == "Halvo" {
                        main_visible = true;
                    }
                    let mut rc = RECT::default();
                    unsafe {
                        let _ = GetWindowRect(h, &mut rc);
                    }
                    let kind = format!("{}:{}x{}", w.class.replace(' ', "_"), rc.right - rc.left, rc.bottom - rc.top);
                    if !visible_kinds.contains(&kind) {
                        visible_kinds.push(kind);
                    }
                }
            }
            let end = match loaded_at {
                Some(l) => l + Duration::from_secs(idle),
                None => Duration::from_secs(90 + idle),
            };
            if t0.elapsed() >= end || wait(h.0, 0) {
                break;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        let alive = !wait(h.0, 0);
        let sum_peaks: u64 = t.peak_by_pid.values().sum();
        println!(
            "S7 app_run label={label} run={run} alive_at_end={alive} window_created={window_seen} window_visible_ever={window_visible} main_window_visible_ever={main_visible} visible_window_kinds={} page_loaded={} page_load_ms={} samples={} max_tree_processes={} max_webview_processes={} processes_seen={} max_summed_working_set_bytes={} sum_of_per_process_peak_working_set_bytes={} app_process_peak_working_set_bytes={} max_summed_private_bytes={} unreadable_reads={}",
            if visible_kinds.is_empty() { "-".to_string() } else { visible_kinds.join(",") },
            loaded_at.is_some(),
            loaded_at.map(|d| d.as_millis().to_string()).unwrap_or("-".into()),
            t.samples,
            t.max_procs,
            t.max_webview,
            t.peak_by_pid.len(),
            t.max_sum_ws,
            sum_peaks,
            t.app_peak,
            t.max_sum_private,
            t.unreadable
        );
        if do_walk && alive {
            walk_app(pid, &label, run);
        }
        let before = descendants(pid, &snapshot());
        terminate(h.0);
        wait(h.0, 10000);
        let mut left = 0;
        for _ in 0..40 {
            let now: HashSet<u32> = snapshot().iter().map(|p| p.pid).collect();
            left = before.iter().filter(|p| **p != pid && now.contains(p)).count();
            if left == 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        println!("S7 app_end label={label} run={run} tree_processes_before_kill={} left_after_10s={left}", before.len());
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Outcome {
    Reached(u32),
    Top,
    ParentGone,
    Unreadable,
    Reused,
    Bound,
}

impl Outcome {
    fn key(&self) -> String {
        match self {
            Outcome::Reached(d) => format!("reached_d{d}"),
            Outcome::Top => "top".into(),
            Outcome::ParentGone => "parent_gone".into(),
            Outcome::Unreadable => "unreadable".into(),
            Outcome::Reused => "reused".into(),
            Outcome::Bound => "bound".into(),
        }
    }
}

/// A bounded walk up the parent links of `pid` to `root`. Each link is checked: the parent
/// exists in the snapshot and was created no later than the child.
pub fn walk(pid: u32, root: u32, root_ct: Option<u64>, ppid: &HashMap<u32, u32>, ct: &mut HashMap<u32, Option<u64>>, bound: u32) -> Outcome {
    let mut cur = pid;
    for depth in 0..=bound {
        if cur == root {
            let c = *ct.entry(cur).or_insert_with(|| creation_time(cur));
            return if c.is_some() && c == root_ct { Outcome::Reached(depth) } else { Outcome::Reused };
        }
        let Some(&p) = ppid.get(&cur) else { return Outcome::ParentGone };
        if p == 0 || p == 4 || p == cur {
            return Outcome::Top;
        }
        if !ppid.contains_key(&p) {
            return Outcome::ParentGone;
        }
        let cc = *ct.entry(cur).or_insert_with(|| creation_time(cur));
        let pc = *ct.entry(p).or_insert_with(|| creation_time(p));
        match (cc, pc) {
            (Some(a), Some(b)) => {
                if b > a {
                    return Outcome::Reused;
                }
            }
            _ => return Outcome::Unreadable,
        }
        cur = p;
    }
    Outcome::Bound
}

fn hist(v: &[Outcome]) -> String {
    let mut m: Vec<(String, usize)> = Vec::new();
    for o in v {
        let k = o.key();
        if let Some(e) = m.iter_mut().find(|e| e.0 == k) {
            e.1 += 1;
        } else {
            m.push((k, 1));
        }
    }
    m.sort();
    m.iter().map(|(k, n)| format!("{k}:{n}")).collect::<Vec<_>>().join(",")
}

pub fn walk_app(root: u32, label: &str, run: usize) {
    let bound = 16;
    let procs = snapshot();
    let ppid: HashMap<u32, u32> = procs.iter().map(|p| (p.pid, p.ppid)).collect();
    let mut ct: HashMap<u32, Option<u64>> = HashMap::new();
    let root_ct = creation_time(root);
    let tree = descendants(root, &procs);
    let tree_set: HashSet<u32> = tree.iter().copied().collect();
    let wins = windows_all(true);
    let names: HashMap<u32, &str> = procs.iter().map(|p| (p.pid, p.exe.as_str())).collect();

    let app_top: Vec<Outcome> = wins.iter().filter(|w| w.top && w.pid == root).map(|w| walk(w.pid, root, root_ct, &ppid, &mut ct, bound)).collect();
    let tree_wins: Vec<&Win> = wins.iter().filter(|w| tree_set.contains(&w.pid) && w.pid != root).collect();
    let tree_win_out: Vec<Outcome> = tree_wins.iter().map(|w| walk(w.pid, root, root_ct, &ppid, &mut ct, bound)).collect();
    let wv: Vec<u32> = tree.iter().copied().filter(|p| names.get(p).map(|n| n.eq_ignore_ascii_case("msedgewebview2.exe")).unwrap_or(false)).collect();
    let wv_out: Vec<Outcome> = wv.iter().map(|&p| walk(p, root, root_ct, &ppid, &mut ct, bound)).collect();
    let all_out: Vec<Outcome> = procs.iter().filter(|p| p.pid != 0 && p.pid != 4).map(|p| walk(p.pid, root, root_ct, &ppid, &mut ct, bound)).collect();
    let windowed: HashSet<u32> = wins.iter().filter(|w| w.top && w.visible).map(|w| w.pid).collect();
    let win_out: Vec<Outcome> = windowed.iter().map(|&p| walk(p, root, root_ct, &ppid, &mut ct, bound)).collect();
    let all_unread = all_out.iter().filter(|o| matches!(o, Outcome::Unreadable | Outcome::Bound)).count();
    let win_unread = win_out.iter().filter(|o| matches!(o, Outcome::Unreadable | Outcome::Bound)).count();
    println!(
        "S9 walk label={label} run={run} bound={bound} app_top_windows={} app_top_outcomes={} webview_child_windows={} webview_child_window_outcomes={} webview_processes={} webview_process_outcomes={} system_processes={} system_outcomes={} system_unreadable={all_unread} visible_window_processes={} visible_window_outcomes={} visible_window_unreadable={win_unread}",
        app_top.len(),
        hist(&app_top),
        tree_wins.len(),
        hist(&tree_win_out),
        wv.len(),
        hist(&wv_out),
        all_out.len(),
        hist(&all_out),
        windowed.len(),
        hist(&win_out)
    );

    // The app's window brought to the front: the foreground window, the window under the
    // centre of the app's window and the focused window, each walked.
    let main = wins.iter().find(|w| w.top && w.pid == root && w.class.starts_with("Tauri")).or_else(|| wins.iter().find(|w| w.top && w.pid == root));
    if let Some(m) = main {
        let h = hwnd(m.hwnd);
        unsafe {
            let _ = ShowWindow(h, SW_SHOWNORMAL);
            let _ = SetWindowPos(h, Some(HWND_TOPMOST), 1160, 60, 600, 600, SWP_SHOWWINDOW);
            let _ = SetForegroundWindow(h);
        }
        std::thread::sleep(Duration::from_millis(1500));
        let fg = unsafe { GetForegroundWindow() };
        let fg_pid = pid_of(fg);
        let fg_out = walk(fg_pid, root, root_ct, &ppid, &mut ct, bound);
        let mut rc = RECT::default();
        unsafe {
            let _ = GetWindowRect(h, &mut rc);
        }
        let pt = POINT { x: (rc.left + rc.right) / 2, y: (rc.top + rc.bottom) / 2 };
        let under = unsafe { WindowFromPoint(pt) };
        let under_pid = pid_of(under);
        // A process that started after the snapshot needs a fresh one.
        let procs2 = snapshot();
        let ppid2: HashMap<u32, u32> = procs2.iter().map(|p| (p.pid, p.ppid)).collect();
        let under_out = walk(under_pid, root, root_ct, &ppid2, &mut ct, bound);
        let tid = unsafe { GetWindowThreadProcessId(fg, None) };
        let mut gi = GUITHREADINFO { cbSize: std::mem::size_of::<GUITHREADINFO>() as u32, ..Default::default() };
        let focus_ok = unsafe { GetGUIThreadInfo(tid, &mut gi).is_ok() };
        let focus_pid = if focus_ok { pid_of(gi.hwndFocus) } else { 0 };
        let focus_out = if focus_pid != 0 { walk(focus_pid, root, root_ct, &ppid2, &mut ct, bound).key() } else { "-".into() };
        println!(
            "S9 front label={label} run={run} foreground_is_app_window={} foreground_outcome={} under_centre_is_app_process={} under_centre_is_webview_process={} under_centre_outcome={} focus_known={} focus_is_app_process={} focus_is_webview_process={} focus_outcome={focus_out}",
            fg == h,
            fg_out.key(),
            under_pid == root,
            names.get(&under_pid).map(|n| n.eq_ignore_ascii_case("msedgewebview2.exe")).unwrap_or(false) || procs2.iter().any(|p| p.pid == under_pid && p.exe.eq_ignore_ascii_case("msedgewebview2.exe")),
            under_out.key(),
            focus_pid != 0,
            focus_pid == root,
            procs2.iter().any(|p| p.pid == focus_pid && p.exe.eq_ignore_ascii_case("msedgewebview2.exe")),
        );
        unsafe {
            let _ = ShowWindow(h, SW_HIDE);
        }
    } else {
        println!("S9 front label={label} run={run} app_window_found=false");
    }
}

/// Peak working set of one command, sampled every 100 ms until it exits.
pub fn peak(args: &[String]) {
    let label = arg(args, "--label").unwrap_or_else(|| "peak".into());
    let cmd = arg(args, "--cmd").unwrap();
    let Ok((h, _pid)) = spawn(&cmd, PROCESS_CREATION_FLAGS(0)) else {
        println!("S8 peak label={label} started=false");
        return;
    };
    let mut pk = 0u64;
    let mut pv = 0u64;
    let mut n = 0;
    loop {
        if let Some((_, p, private)) = memory(h.0) {
            pk = pk.max(p);
            pv = pv.max(private);
        }
        n += 1;
        if wait(h.0, 100) {
            break;
        }
    }
    if let Some((_, p, private)) = memory(h.0) {
        pk = pk.max(p);
        pv = pv.max(private);
    }
    println!("S8 peak label={label} exit_code={} samples={n} peak_working_set_bytes={pk} max_private_bytes={pv}", exit_code(h.0).unwrap_or(u32::MAX));
}
