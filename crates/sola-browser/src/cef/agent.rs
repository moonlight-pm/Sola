//! Helper-side CDP wrap for agent snapshot/act. Not an agent API.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::mpsc::Sender;

use cef::rc::*;
use cef::*;
use cef::ImplBrowserHost;

use crate::agent::{AgentOp, AgentReply, AgentRequest};
use crate::ax::{self, SnapshotOpts};
use crate::cef::ipc::FromEngine;

thread_local! {
    static JOBS: RefCell<HashMap<(i32, i32), Job>> = RefCell::new(HashMap::new());
    static IPC: RefCell<Option<Sender<FromEngine>>> = const { RefCell::new(None) };
}

struct Job {
    req: AgentRequest,
    phase: Phase,
}

enum Phase {
    AxEnable,
    AxTree,
    Resolve { next: AfterResolve },
    CallJs,
    BoxModel,
    Mouse {
        x: i32,
        y: i32,
        step: u8,
    },
    Ready,
    Shot { path: String },
}

enum AfterResolve {
    Click { role: String, name: String },
    #[allow(dead_code)]
    Hover { role: String, name: String },
    Type {
        role: String,
        name: String,
        text: String,
        submit: bool,
    },
    Fill {
        role: String,
        name: String,
        text: String,
    },
    Select {
        role: String,
        name: String,
        values: Vec<String>,
    },
}

pub fn init(ipc: Sender<FromEngine>) {
    IPC.with(|s| *s.borrow_mut() = Some(ipc));
}

pub fn attach(host: &mut BrowserHost) -> Option<Registration> {
    let mut obs = AgentDevToolsObserver::new(());
    host.add_dev_tools_message_observer(Some(&mut obs))
}

pub fn begin(host: &BrowserHost, browser_id: i32, req: AgentRequest) {
    let tab = req.tab;
    let id = req.id;
    match req.op.clone() {
        AgentOp::Snapshot { .. } => {
            start(
                host,
                browser_id,
                Job {
                    req,
                    phase: Phase::AxEnable,
                },
            );
        }
        AgentOp::Click {
            backend_node_id,
            role,
            name,
        } => {
            resolve(
                host,
                browser_id,
                req,
                backend_node_id,
                AfterResolve::Click { role, name },
            );
        }
        AgentOp::Hover {
            backend_node_id, ..
        } => {
            let mut params = dict();
            set_int(&mut params, "backendNodeId", backend_node_id);
            let mid = cdp(host, "DOM.getBoxModel", Some(&mut params));
            put(
                browser_id,
                mid,
                Job {
                    req,
                    phase: Phase::BoxModel,
                },
            );
        }
        AgentOp::Type {
            backend_node_id,
            role,
            name,
            text,
            submit,
        } => {
            resolve(
                host,
                browser_id,
                req,
                backend_node_id,
                AfterResolve::Type {
                    role,
                    name,
                    text,
                    submit,
                },
            );
        }
        AgentOp::Fill {
            backend_node_id,
            role,
            name,
            text,
        } => {
            resolve(
                host,
                browser_id,
                req,
                backend_node_id,
                AfterResolve::Fill { role, name, text },
            );
        }
        AgentOp::Select {
            backend_node_id,
            role,
            name,
            values,
        } => {
            resolve(
                host,
                browser_id,
                req,
                backend_node_id,
                AfterResolve::Select {
                    role,
                    name,
                    values,
                },
            );
        }
        AgentOp::Screenshot { path } => {
            let mut params = dict();
            set_str(&mut params, "format", "png");
            let mid = cdp(host, "Page.captureScreenshot", Some(&mut params));
            put(
                browser_id,
                mid,
                Job {
                    phase: Phase::Shot { path },
                    req,
                },
            );
        }
        AgentOp::ReadyState => {
            let mut params = dict();
            set_str(
                &mut params,
                "expression",
                "({ready: document.readyState, url: location.href, title: document.title})",
            );
            set_bool(&mut params, "returnByValue", true);
            let mid = cdp(host, "Runtime.evaluate", Some(&mut params));
            put(
                browser_id,
                mid,
                Job {
                    req,
                    phase: Phase::Ready,
                },
            );
        }
        AgentOp::Nav(_) | AgentOp::FindPage { .. } => {
            emit(AgentReply {
                id,
                tab,
                ok: true,
                error: None,
                yaml: None,
                refs: Vec::new(),
                url: None,
                title: None,
                focused: None,
                dialog_open: false,
                json: false,
                path: None,
                ready: None,
            });
        }
    }
}

fn start(host: &BrowserHost, browser_id: i32, job: Job) {
    let mid = cdp(host, "Accessibility.enable", None);
    put(
        browser_id,
        mid,
        Job {
            phase: Phase::AxEnable,
            req: job.req,
        },
    );
}

fn resolve(host: &BrowserHost, browser_id: i32, req: AgentRequest, backend: i32, next: AfterResolve) {
    let mut params = dict();
    set_int(&mut params, "backendNodeId", backend);
    let mid = cdp(host, "DOM.resolveNode", Some(&mut params));
    put(
        browser_id,
        mid,
        Job {
            req,
            phase: Phase::Resolve { next },
        },
    );
}

fn put(browser_id: i32, message_id: i32, job: Job) {
    if message_id <= 0 {
        emit(AgentReply::fail(
            job.req.id,
            job.req.tab,
            "devtools method failed to submit",
        ));
        return;
    }
    JOBS.with(|j| {
        j.borrow_mut().insert((browser_id, message_id), job);
    });
}

fn cdp(host: &BrowserHost, method: &str, params: Option<&mut DictionaryValue>) -> i32 {
    let m = CefString::from(method);
    host.execute_dev_tools_method(0, Some(&m), params)
}

fn dict() -> DictionaryValue {
    dictionary_value_create().expect("cef dictionary")
}

fn set_str(d: &mut DictionaryValue, k: &str, v: &str) {
    let key = CefString::from(k);
    let val = CefString::from(v);
    let _ = d.set_string(Some(&key), Some(&val));
}

fn set_int(d: &mut DictionaryValue, k: &str, v: i32) {
    let key = CefString::from(k);
    let _ = d.set_int(Some(&key), v);
}

fn set_bool(d: &mut DictionaryValue, k: &str, v: bool) {
    let key = CefString::from(k);
    let _ = d.set_bool(Some(&key), v as _);
}

fn emit(r: AgentReply) {
    crate::cef::engine::conceal_agent_tab();
    IPC.with(|s| {
        if let Some(tx) = s.borrow().as_ref() {
            let _ = tx.send(FromEngine::Agent(r));
        }
    });
}

fn on_result(browser_id: i32, message_id: i32, success: bool, result: &str) {
    let job = JOBS.with(|j| j.borrow_mut().remove(&(browser_id, message_id)));
    let Some(job) = job else {
        return;
    };
    if !success {
        emit(AgentReply::fail(
            job.req.id,
            job.req.tab,
            format!("cdp error: {result}"),
        ));
        return;
    }
    let host = current_host(browser_id);
    let Some(host) = host else {
        emit(AgentReply::fail(job.req.id, job.req.tab, "tab gone"));
        return;
    };
    match job.phase {
        Phase::AxEnable => {
            let mid = cdp(&host, "Accessibility.getFullAXTree", None);
            put(
                browser_id,
                mid,
                Job {
                    req: job.req,
                    phase: Phase::AxTree,
                },
            );
        }
        Phase::AxTree => finish_snapshot(job.req, result),
        Phase::Resolve { next } => {
            let object_id = object_id(result);
            let Some(object_id) = object_id else {
                emit(AgentReply::fail(
                    job.req.id,
                    job.req.tab,
                    "ref not found in the current page snapshot. Try capturing new snapshot.",
                ));
                return;
            };
            let (js, role, name) = js_for(&next);
            let mut params = dict();
            set_str(&mut params, "objectId", &object_id);
            set_str(&mut params, "functionDeclaration", &js);
            set_bool(&mut params, "returnByValue", true);
            let mid = cdp(&host, "Runtime.callFunctionOn", Some(&mut params));
            put(
                browser_id,
                mid,
                Job {
                    req: job.req,
                    phase: Phase::CallJs,
                },
            );
            let _ = (role, name);
        }
        Phase::CallJs => {
            match cdp_js_value(result) {
                Err(e) => {
                    emit(AgentReply::fail(job.req.id, job.req.tab, e));
                    return;
                }
                Ok(v) if is_stale_value(&v) => {
                    emit(AgentReply::fail(
                        job.req.id,
                        job.req.tab,
                        "stale ref; snapshot again",
                    ));
                    return;
                }
                Ok(v) => {
                    if matches!(job.req.op, AgentOp::Click { .. }) {
                        if v.get("clicked").and_then(|c| c.as_bool()) != Some(true) {
                            if let Some((x, y)) = pointer_target(&v) {
                                queue_mouse(
                                    &host,
                                    browser_id,
                                    job.req,
                                    x,
                                    y,
                                    "mouseMoved",
                                    false,
                                    1,
                                );
                                return;
                            } else if let Some(href) = v.get("href").and_then(|s| s.as_str()) {
                                if !href.is_empty() && !href.starts_with("javascript:") {
                                    crate::cef::engine::agent_load_url(browser_id, href);
                                }
                            }
                        }
                    }
                }
            }
            emit_ok(job.req.id, job.req.tab, None);
        }
        Phase::BoxModel => {
            let Some((x, y)) = box_center(result) else {
                emit(AgentReply::fail(
                    job.req.id,
                    job.req.tab,
                    "element has no box (hidden?); snapshot again",
                ));
                return;
            };
            queue_mouse(
                &host,
                browser_id,
                job.req,
                x,
                y,
                "mouseMoved",
                false,
                3,
            );
        }
        Phase::Mouse { x, y, step } => match step {
            1 => queue_mouse(&host, browser_id, job.req, x, y, "mousePressed", true, 2),
            2 => queue_mouse(&host, browser_id, job.req, x, y, "mouseReleased", false, 3),
            _ => emit_ok(job.req.id, job.req.tab, None),
        },
        Phase::Ready => match parse_ready(result) {
            Ok((ready, url, title)) => {
                emit(AgentReply {
                    id: job.req.id,
                    tab: job.req.tab,
                    ok: true,
                    error: None,
                    yaml: None,
                    refs: Vec::new(),
                    url: Some(url),
                    title: Some(title),
                    focused: None,
                    dialog_open: false,
                    json: false,
                    path: None,
                    ready: Some(ready),
                });
            }
            Err(e) => emit(AgentReply::fail(job.req.id, job.req.tab, e)),
        },
        Phase::Shot { path } => {
            if let Err(e) = write_png(result, Path::new(&path)) {
                emit(AgentReply::fail(job.req.id, job.req.tab, e));
                return;
            }
            emit_ok(job.req.id, job.req.tab, Some(path));
        }
    }
}

fn finish_snapshot(req: AgentRequest, json: &str) {
    let AgentOp::Snapshot {
        interactive,
        subtree_backend,
        json: as_json,
    } = req.op
    else {
        emit(AgentReply::fail(req.id, req.tab, "not a snapshot"));
        return;
    };
    let nodes = match ax::parse_cdp_nodes(json) {
        Ok(n) => n,
        Err(e) => {
            emit(AgentReply::fail(req.id, req.tab, e));
            return;
        }
    };
    let snap = match ax::snapshot_from_cdp(
        &nodes,
        SnapshotOpts {
            interactive,
            subtree_backend,
            url: String::new(),
            title: String::new(),
            frame: 0,
        },
    ) {
        Ok(s) => s,
        Err(e) => {
            emit(AgentReply::fail(req.id, req.tab, e));
            return;
        }
    };
    emit(AgentReply {
        id: req.id,
        tab: req.tab,
        ok: true,
        error: None,
        yaml: Some(snap.yaml),
        refs: snap.refs,
        url: None,
        title: None,
        focused: snap.focused,
        dialog_open: snap.dialog_open,
        json: as_json,
        path: None,
        ready: None,
    });
}

fn object_id(json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    v.get("object")
        .and_then(|o| o.get("objectId"))
        .and_then(|s| s.as_str())
        .map(str::to_string)
}

fn box_center(json: &str) -> Option<(i32, i32)> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let content = v.get("model")?.get("content")?.as_array()?;
    let xs: Vec<f64> = (0..4)
        .filter_map(|i| content.get(i * 2)?.as_f64())
        .collect();
    let ys: Vec<f64> = (0..4)
        .filter_map(|i| content.get(i * 2 + 1)?.as_f64())
        .collect();
    if xs.len() != 4 || ys.len() != 4 {
        return None;
    }
    let x = xs.iter().sum::<f64>() / 4.0;
    let y = ys.iter().sum::<f64>() / 4.0;
    Some((x.round() as i32, y.round() as i32))
}

fn write_png(json: &str, path: &Path) -> Result<(), String> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("screenshot json: {e}"))?;
    let b64 = v
        .get("data")
        .and_then(|d| d.as_str())
        .ok_or_else(|| "screenshot missing data".to_string())?;
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| format!("screenshot b64: {e}"))?;
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    fs::write(path, bytes).map_err(|e| format!("write screenshot: {e}"))
}

fn js_for(next: &AfterResolve) -> (String, String, String) {
    match next {
        AfterResolve::Click { role, name } => (
            format!(
                "function() {{ const el = {pick}; if (!el) return {{stale:true}}; if ({stale}) return {{stale:true}}; el.scrollIntoView({{block:'center',inline:'nearest'}}); const href = (el.href || (el.getAttribute && el.getAttribute('href')) || ''); const r = el.getBoundingClientRect(); if (r.width < 1 || r.height < 1) {{ if (href && href.indexOf('javascript:') !== 0) return {{ok:true, href: href}}; if (typeof el.click === 'function') el.click(); return {{ok:true, clicked:true}}; }} return {{ok:true, x: r.left + r.width/2, y: r.top + r.height/2, href: href}}; }}",
                pick = pick_el(),
                stale = stale_check(name, role),
            ),
            role.clone(),
            name.clone(),
        ),
        AfterResolve::Hover { role, name } => (
            format!(
                "function() {{ const el = {pick}; if (!el) return {{stale:true}}; if ({stale}) return {{stale:true}}; el.scrollIntoView({{block:'center',inline:'nearest'}}); el.dispatchEvent(new MouseEvent('mouseover',{{bubbles:true}})); return {{ok:true}}; }}",
                pick = pick_el(),
                stale = stale_check(name, role),
            ),
            role.clone(),
            name.clone(),
        ),
        AfterResolve::Type {
            role,
            name,
            text,
            submit,
        } => {
            let lit = serde_json::to_string(text).unwrap_or_else(|_| "\"\"".into());
            let enter = if *submit {
                "el.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true})); if (el.form) { try { el.form.requestSubmit(); } catch(e) { el.form.submit(); } }"
            } else {
                ""
            };
            (
                format!(
                    "function() {{ const el = {pick}; if (!el) return {{stale:true}}; if ({stale}) return {{stale:true}}; const v = {lit}; el.focus(); {set} el.dispatchEvent(new Event('input',{{bubbles:true}})); el.dispatchEvent(new Event('change',{{bubbles:true}})); {enter} return {{ok:true}}; }}",
                    pick = pick_el(),
                    stale = stale_check(name, role),
                    set = native_set(true),
                    enter = enter,
                ),
                role.clone(),
                name.clone(),
            )
        }
        AfterResolve::Fill { role, name, text } => {
            let lit = serde_json::to_string(text).unwrap_or_else(|_| "\"\"".into());
            (
                format!(
                    "function() {{ const el = {pick}; if (!el) return {{stale:true}}; if ({stale}) return {{stale:true}}; const v = {lit}; el.focus(); {set} el.dispatchEvent(new Event('input',{{bubbles:true}})); el.dispatchEvent(new Event('change',{{bubbles:true}})); return {{ok:true}}; }}",
                    pick = pick_el(),
                    stale = stale_check(name, role),
                    set = native_set(false),
                ),
                role.clone(),
                name.clone(),
            )
        }
        AfterResolve::Select {
            role,
            name,
            values,
        } => {
            let lit = serde_json::to_string(values).unwrap_or_else(|_| "[]".into());
            (
                format!(
                    "function() {{ const el = {pick}; if (!el) return {{stale:true}}; if ({stale}) return {{stale:true}}; const wanted = new Set({lit}); if (el.options) {{ for (const o of el.options) o.selected = wanted.has(o.value) || wanted.has(o.text); el.dispatchEvent(new Event('change',{{bubbles:true}})); }} return {{ok:true}}; }}",
                    pick = pick_el(),
                    stale = stale_check(name, role),
                ),
                role.clone(),
                name.clone(),
            )
        }
    }
}

fn pick_el() -> &'static str {
    "(function(n){ let el = n; if (el && el.nodeType === 3) el = el.parentElement; if (!el) return null; return (el.closest && el.closest('a,button,input,select,textarea,summary,[role=\"link\"],[role=\"button\"]')) || el; })(this)"
}

fn native_set(append: bool) -> &'static str {
    if append {
        "if (el.isContentEditable) { el.textContent = (el.textContent||'') + v; } else { const proto = (el.tagName === 'TEXTAREA') ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype; const desc = Object.getOwnPropertyDescriptor(proto, 'value'); const next = String(el.value||'') + v; if (desc && desc.set) desc.set.call(el, next); else if ('value' in el) el.value = next; }"
    } else {
        "if (el.isContentEditable) { el.textContent = v; } else { const proto = (el.tagName === 'TEXTAREA') ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype; const desc = Object.getOwnPropertyDescriptor(proto, 'value'); if (desc && desc.set) desc.set.call(el, v); else if ('value' in el) el.value = v; }"
    }
}

fn stale_check(name: &str, role: &str) -> String {
    let name_lit = serde_json::to_string(name).unwrap_or_else(|_| "\"\"".into());
    let role_lit = serde_json::to_string(role).unwrap_or_else(|_| "\"\"".into());
    format!(
        "(function(){{ const n = {name_lit}.trim(); const role = {role_lit}; const label = el.labels && el.labels[0] ? el.labels[0].innerText : ''; const raw = ((el.getAttribute && el.getAttribute('aria-label')) || el.innerText || label || ((role === 'textbox' || role === 'searchbox') ? el.value : '') || el.textContent || '').trim(); const got = raw.toLowerCase(); const want = n.toLowerCase(); if (want && got && got.indexOf(want) === -1 && want.indexOf(got) === -1) return true; if (role === 'link' && !(el.closest && el.closest('a,[role=\"link\"]'))) return true; if (role === 'button' && !(el.closest && el.closest('button,[role=\"button\"],input[type=\"button\"],input[type=\"submit\"]'))) return true; return false; }})()"
    )
}

fn emit_ok(id: u64, tab: u64, path: Option<String>) {
    emit(AgentReply {
        id,
        tab,
        ok: true,
        error: None,
        yaml: None,
        refs: Vec::new(),
        url: None,
        title: None,
        focused: None,
        dialog_open: false,
        json: false,
        path,
        ready: None,
    });
}

fn queue_mouse(
    host: &BrowserHost,
    browser_id: i32,
    req: AgentRequest,
    x: i32,
    y: i32,
    ty: &str,
    down: bool,
    next: u8,
) {
    let mid = dispatch_mouse(host, ty, x, y, down);
    put(
        browser_id,
        mid,
        Job {
            req,
            phase: Phase::Mouse { x, y, step: next },
        },
    );
}

fn dispatch_mouse(host: &BrowserHost, ty: &str, x: i32, y: i32, down: bool) -> i32 {
    let mut params = dict();
    set_str(&mut params, "type", ty);
    set_int(&mut params, "x", x);
    set_int(&mut params, "y", y);
    set_str(&mut params, "button", "left");
    set_int(&mut params, "buttons", if down { 1 } else { 0 });
    set_int(
        &mut params,
        "clickCount",
        if ty == "mouseMoved" { 0 } else { 1 },
    );
    cdp(host, "Input.dispatchMouseEvent", Some(&mut params))
}

fn cdp_js_value(json: &str) -> Result<serde_json::Value, String> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("cdp json: {e}"))?;
    if let Some(exc) = v.get("exceptionDetails") {
        let msg = exc
            .pointer("/exception/description")
            .and_then(|t| t.as_str())
            .or_else(|| exc.get("text").and_then(|t| t.as_str()))
            .unwrap_or("javascript exception");
        return Err(msg.to_string());
    }
    Ok(v.get("result")
        .and_then(|r| r.get("value"))
        .cloned()
        .unwrap_or(v))
}

fn is_stale_value(v: &serde_json::Value) -> bool {
    v.get("stale").and_then(|s| s.as_bool()) == Some(true)
}

fn pointer_target(v: &serde_json::Value) -> Option<(i32, i32)> {
    let x = v.get("x")?.as_f64()?;
    let y = v.get("y")?.as_f64()?;
    Some((x.round() as i32, y.round() as i32))
}

fn parse_ready(json: &str) -> Result<(String, String, String), String> {
    let v = cdp_js_value(json)?;
    let ready = v
        .get("ready")
        .and_then(|s| s.as_str())
        .unwrap_or("loading")
        .to_string();
    let url = v
        .get("url")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_string();
    let title = v
        .get("title")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_string();
    Ok((ready, url, title))
}

fn current_host(browser_id: i32) -> Option<BrowserHost> {
    use crate::cef::engine::host_for_browser_id;
    host_for_browser_id(browser_id)
}

cef::wrap_dev_tools_message_observer! {
    struct AgentDevToolsObserver {
        marker: (),
    }

    impl DevToolsMessageObserver {
        fn on_dev_tools_method_result(
            &self,
            browser: Option<&mut Browser>,
            message_id: ::std::os::raw::c_int,
            success: ::std::os::raw::c_int,
            result: Option<&[u8]>,
        ) {
            let bid = browser.map(|b| b.identifier()).unwrap_or(0);
            let json = result
                .map(|b| String::from_utf8_lossy(b).into_owned())
                .unwrap_or_default();
            on_result(bid, message_id, success != 0, &json);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cdp_js_value_reads_return_by_value() {
        let v = cdp_js_value(r#"{"result":{"type":"object","value":{"ok":true,"x":10.4,"y":20.6}}}"#)
            .unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(pointer_target(&v), Some((10, 21)));
    }

    #[test]
    fn cdp_js_value_surfaces_exception() {
        let err = cdp_js_value(
            r#"{"result":{"type":"undefined"},"exceptionDetails":{"text":"Uncaught","exception":{"description":"TypeError: this.click is not a function"}}}"#,
        )
        .unwrap_err();
        assert!(err.contains("TypeError"), "{err}");
    }

    #[test]
    fn stale_object_is_detected() {
        let v = serde_json::json!({"stale": true});
        assert!(is_stale_value(&v));
        assert!(!is_stale_value(&serde_json::json!({"ok": true})));
    }

    #[test]
    fn parse_ready_from_evaluate() {
        let (ready, url, title) = parse_ready(
            r#"{"result":{"type":"object","value":{"ready":"complete","url":"https://example.com/","title":"Example Domain"}}}"#,
        )
        .unwrap();
        assert_eq!(ready, "complete");
        assert_eq!(url, "https://example.com/");
        assert_eq!(title, "Example Domain");
    }
}
