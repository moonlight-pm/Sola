//! CalDAV: discover calendars, fetch VEVENTs, PUT / DELETE.
//! iCloud is the default base; generic hosts pass `discover_at`.

use anyhow::{Result, anyhow, bail};
use chrono::{Datelike, NaiveDate};
use quick_xml::Reader;
use quick_xml::events::Event;
use reqwest::Method;
use url::Url;

use crate::ics::{emit_vevent, parse_vevents, to_cal_events};
use crate::model::{CalEvent, CalKind, Calendar};

const ICLOUD: &str = "https://caldav.icloud.com/";
const NS_DAV: &str = "DAV:";
const NS_CAL: &str = "urn:ietf:params:xml:ns:caldav";

pub async fn discover(
    http: &reqwest::Client,
    apple_id: &str,
    password: &str,
    account_id: &str,
) -> Result<(String, Vec<Calendar>)> {
    discover_at(http, apple_id, password, account_id, ICLOUD, CalKind::Apple).await
}

pub async fn discover_at(
    http: &reqwest::Client,
    user: &str,
    password: &str,
    account_id: &str,
    base_url: &str,
    kind: CalKind,
) -> Result<(String, Vec<Calendar>)> {
    let base = if base_url.trim().is_empty() {
        ICLOUD
    } else {
        base_url.trim()
    };
    let principal = current_user_principal(http, user, password, base).await?;
    let home = calendar_home_set(http, user, password, &principal).await?;
    let calendars = list_calendars(http, user, password, account_id, &home, kind).await?;
    Ok((principal, calendars))
}

pub async fn fetch_events(
    http: &reqwest::Client,
    apple_id: &str,
    password: &str,
    calendar: &Calendar,
    window: (NaiveDate, NaiveDate),
) -> Result<Vec<CalEvent>> {
    let Some(href) = calendar.href.as_deref() else {
        return Ok(Vec::new());
    };
    let start = format!("{}T000000Z", ymd(window.0));
    let end = format!(
        "{}T000000Z",
        ymd(window.1 + chrono::Duration::days(1))
    );
    let body = format!(
        r#"<?xml version="1.0" encoding="utf-8" ?>
<c:calendar-query xmlns:d="DAV:" xmlns:c="{NS_CAL}">
  <d:prop>
    <d:getetag/>
    <c:calendar-data/>
  </d:prop>
  <c:filter>
    <c:comp-filter name="VCALENDAR">
      <c:comp-filter name="VEVENT">
        <c:time-range start="{start}" end="{end}"/>
      </c:comp-filter>
    </c:comp-filter>
  </c:filter>
</c:calendar-query>"#
    );
    let xml = dav(http, apple_id, password, "REPORT", href, 1, &body).await?;
    let mut events = Vec::new();
    for resp in parse_responses(&xml) {
        if resp.status >= 400 {
            continue;
        }
        let Some(data) = resp.calendar_data else {
            continue;
        };
        for raw in parse_vevents(&data) {
            events.extend(to_cal_events(
                raw,
                &calendar.id,
                Some(resp.href.clone()),
                resp.etag.clone(),
                window,
            ));
        }
    }
    Ok(events)
}

pub async fn put_event(
    http: &reqwest::Client,
    apple_id: &str,
    password: &str,
    calendar: &Calendar,
    event: &CalEvent,
) -> Result<String> {
    let cal_href = calendar
        .href
        .as_deref()
        .ok_or_else(|| anyhow!("missing calendar href"))?;
    let href = if let Some(existing) = event.href.as_deref().filter(|s| !s.is_empty()) {
        existing.to_string()
    } else {
        join_href(cal_href, &format!("{}.ics", event.id.replace(':', "-")))
    };
    let body = emit_vevent(event);
    let response = http
        .request(Method::from_bytes(b"PUT").unwrap(), &href)
        .header("Content-Type", "text/calendar; charset=utf-8")
        .basic_auth(apple_id, Some(password))
        .body(body)
        .send()
        .await?;
    let status = response.status();
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        bail!("CalDAV PUT {href} failed ({status}): {text}");
    }
    Ok(href)
}

pub async fn delete_event(
    http: &reqwest::Client,
    apple_id: &str,
    password: &str,
    event: &CalEvent,
) -> Result<()> {
    let href = event
        .href
        .as_deref()
        .ok_or_else(|| anyhow!("missing event href"))?;
    let response = http
        .delete(href)
        .basic_auth(apple_id, Some(password))
        .send()
        .await?;
    if !response.status().is_success() && response.status().as_u16() != 404 {
        let text = response.text().await.unwrap_or_default();
        bail!("CalDAV DELETE failed: {text}");
    }
    Ok(())
}

async fn current_user_principal(
    http: &reqwest::Client,
    user: &str,
    password: &str,
    url: &str,
) -> Result<String> {
    let body = r#"<?xml version="1.0" encoding="utf-8" ?>
<d:propfind xmlns:d="DAV:">
  <d:prop><d:current-user-principal/></d:prop>
</d:propfind>"#;
    let xml = dav(http, user, password, "PROPFIND", url, 0, body).await?;
    first_href_named(&xml, "current-user-principal")
        .or_else(|| first_href(&xml))
        .map(|h| absolutize(url, &h))
        .ok_or_else(|| anyhow!("CalDAV did not return a principal"))
}

async fn calendar_home_set(
    http: &reqwest::Client,
    user: &str,
    password: &str,
    principal: &str,
) -> Result<String> {
    let body = format!(
        r#"<?xml version="1.0" encoding="utf-8" ?>
<d:propfind xmlns:d="DAV:" xmlns:c="{NS_CAL}">
  <d:prop><c:calendar-home-set/></d:prop>
</d:propfind>"#
    );
    let xml = dav(http, user, password, "PROPFIND", principal, 0, &body).await?;
    first_href_named(&xml, "calendar-home-set")
        .or_else(|| first_href(&xml))
        .map(|h| absolutize(principal, &h))
        .ok_or_else(|| anyhow!("CalDAV did not return a calendar home"))
}

async fn list_calendars(
    http: &reqwest::Client,
    user: &str,
    password: &str,
    account_id: &str,
    home: &str,
    kind: CalKind,
) -> Result<Vec<Calendar>> {
    let body = format!(
        r#"<?xml version="1.0" encoding="utf-8" ?>
<d:propfind xmlns:d="DAV:" xmlns:c="{NS_CAL}" xmlns:a="http://apple.com/ns/ical/">
  <d:prop>
    <d:displayname/>
    <d:resourcetype/>
    <c:supported-calendar-component-set/>
    <a:calendar-color/>
  </d:prop>
</d:propfind>"#
    );
    let xml = dav(http, user, password, "PROPFIND", home, 1, &body).await?;
    let mut out = Vec::new();
    for resp in parse_responses(&xml) {
        if resp.status >= 400 {
            continue;
        }
        if !resp.is_calendar {
            continue;
        }
        if resp.components.iter().any(|c| c.eq_ignore_ascii_case("VTODO"))
            && !resp
                .components
                .iter()
                .any(|c| c.eq_ignore_ascii_case("VEVENT"))
        {
            continue;
        }
        let name = if resp.displayname.trim().is_empty() {
            "Calendar".into()
        } else {
            resp.displayname
        };
        let color = normalize_color(&resp.color).unwrap_or_else(|| "#7aa2f7".into());
        let href = absolutize(home, &resp.href);
        out.push(Calendar {
            id: format!("a-{account_id}-{}", crate::model::new_id("cal")),
            name,
            color,
            kind,
            account_id: Some(account_id.into()),
            visible: true,
            read_only: false,
            remote_id: Some(href.clone()),
            href: Some(href),
        });
    }
    if out.is_empty() {
        bail!("no event calendars found");
    }
    Ok(out)
}

async fn dav(
    http: &reqwest::Client,
    user: &str,
    password: &str,
    method: &str,
    url: &str,
    depth: u8,
    body: &str,
) -> Result<String> {
    let response = http
        .request(Method::from_bytes(method.as_bytes()).unwrap(), url)
        .header("Depth", depth.to_string())
        .header("Content-Type", "application/xml; charset=utf-8")
        .basic_auth(user, Some(password))
        .body(body.to_string())
        .send()
        .await?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() && status.as_u16() != 207 {
        bail!("CalDAV {method} {url} failed ({status}): {text}");
    }
    Ok(text)
}

#[derive(Default)]
struct DavResp {
    href: String,
    status: u16,
    displayname: String,
    etag: Option<String>,
    calendar_data: Option<String>,
    color: String,
    is_calendar: bool,
    components: Vec<String>,
}

fn parse_responses(xml: &str) -> Vec<DavResp> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    let mut out = Vec::new();
    let mut cur = DavResp {
        status: 200,
        ..DavResp::default()
    };
    let mut text = String::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let local = local_name(e.local_name().as_ref());
                stack.push(local.clone());
                text.clear();
                if local == "response" {
                    cur = DavResp {
                        status: 200,
                        ..DavResp::default()
                    };
                }
                if local == "calendar" && stack.iter().any(|s| s == "resourcetype") {
                    cur.is_calendar = true;
                }
                if local == "comp" {
                    if let Some(attr) = e.try_get_attribute(b"name").ok().flatten() {
                        if let Ok(v) = attr.unescape_value() {
                            cur.components.push(v.into_owned());
                        }
                    }
                }
            }
            Ok(Event::Empty(e)) => {
                let local = local_name(e.local_name().as_ref());
                if local == "calendar" && stack.iter().any(|s| s == "resourcetype") {
                    cur.is_calendar = true;
                }
                if local == "comp" {
                    if let Some(attr) = e.try_get_attribute(b"name").ok().flatten() {
                        if let Ok(v) = attr.unescape_value() {
                            cur.components.push(v.into_owned());
                        }
                    }
                }
            }
            Ok(Event::Text(t)) => {
                text.push_str(&t.unescape().unwrap_or_default());
            }
            Ok(Event::CData(t)) => {
                text.push_str(&String::from_utf8_lossy(&t.into_inner()));
            }
            Ok(Event::End(e)) => {
                let local = local_name(e.local_name().as_ref());
                match local.as_str() {
                    "href" => {
                        if !stack.iter().any(|s| {
                            s == "current-user-principal" || s == "calendar-home-set"
                        }) {
                            if cur.href.is_empty() {
                                cur.href = text.trim().to_string();
                            }
                        }
                    }
                    "displayname" => cur.displayname = text.trim().to_string(),
                    "getetag" => cur.etag = Some(text.trim().to_string()),
                    "calendar-data" => cur.calendar_data = Some(text.clone()),
                    "calendar-color" => cur.color = text.trim().to_string(),
                    "status" => {
                        if let Some(code) = text.split_whitespace().nth(1) {
                            if let Ok(n) = code.parse() {
                                cur.status = n;
                            }
                        }
                    }
                    "response" => out.push(std::mem::take(&mut cur)),
                    _ => {}
                }
                stack.pop();
                text.clear();
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    out
}

fn first_href(xml: &str) -> Option<String> {
    texts_named(xml, "href").into_iter().find(|s| !s.is_empty())
}

fn first_href_named(xml: &str, wrapper: &str) -> Option<String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut depth = 0;
    let mut want_href = false;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let local = local_name(e.local_name().as_ref());
                if local == wrapper {
                    depth += 1;
                } else if depth > 0 && local == "href" {
                    want_href = true;
                }
            }
            Ok(Event::Text(t)) if want_href => {
                return Some(t.unescape().unwrap_or_default().trim().to_string());
            }
            Ok(Event::End(e)) => {
                let local = local_name(e.local_name().as_ref());
                if local == wrapper && depth > 0 {
                    depth -= 1;
                }
                if local == "href" {
                    want_href = false;
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    None
}

fn texts_named(xml: &str, local: &str) -> Vec<String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut want = 0;
    let mut current = String::new();
    let mut out = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                if local_name(e.local_name().as_ref()) == local {
                    want += 1;
                    current.clear();
                }
            }
            Ok(Event::Text(t)) if want > 0 => {
                current.push_str(&t.unescape().unwrap_or_default());
            }
            Ok(Event::End(e)) => {
                if local_name(e.local_name().as_ref()) == local && want > 0 {
                    want -= 1;
                    if want == 0 {
                        out.push(current.trim().to_string());
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    out
}

fn local_name(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn absolutize(base: &str, href: &str) -> String {
    if href.starts_with("http://") || href.starts_with("https://") {
        return href.to_string();
    }
    if let Ok(base) = Url::parse(base) {
        if let Ok(joined) = base.join(href) {
            return joined.to_string();
        }
    }
    if href.starts_with('/') {
        "https://caldav.icloud.com".to_string() + href
    } else {
        format!("{base}{href}")
    }
}

fn join_href(cal: &str, file: &str) -> String {
    if cal.ends_with('/') {
        format!("{cal}{file}")
    } else {
        format!("{cal}/{file}")
    }
}

fn ymd(d: NaiveDate) -> String {
    format!("{:04}{:02}{:02}", d.year(), d.month(), d.day())
}

fn normalize_color(s: &str) -> Option<String> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let hex = s.trim_start_matches('#');
    if hex.len() >= 6 {
        Some(format!("#{}", &hex[..6]))
    } else {
        None
    }
}

#[allow(dead_code)]
fn _ns_dav() -> &'static str {
    NS_DAV
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn principal_href_from_wrapper() {
        let xml = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:">
  <d:response>
    <d:href>/</d:href>
    <d:propstat>
      <d:prop>
        <d:current-user-principal>
          <d:href>/12345/principal/</d:href>
        </d:current-user-principal>
      </d:prop>
    </d:propstat>
  </d:response>
</d:multistatus>"#;
        assert_eq!(
            first_href_named(xml, "current-user-principal").as_deref(),
            Some("/12345/principal/")
        );
    }

    #[test]
    fn calendar_list_filters_collections() {
        let xml = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav" xmlns:a="http://apple.com/ns/ical/">
  <d:response>
    <d:href>/home/</d:href>
    <d:propstat>
      <d:prop>
        <d:displayname>Home</d:displayname>
        <d:resourcetype><d:collection/></d:resourcetype>
      </d:prop>
    </d:propstat>
  </d:response>
  <d:response>
    <d:href>/home/work/</d:href>
    <d:propstat>
      <d:prop>
        <d:displayname>Work</d:displayname>
        <d:resourcetype><d:collection/><c:calendar/></d:resourcetype>
        <a:calendar-color>#ff6b6b</a:calendar-color>
      </d:prop>
    </d:propstat>
  </d:response>
</d:multistatus>"#;
        let resps = parse_responses(xml);
        let cals: Vec<_> = resps.into_iter().filter(|r| r.is_calendar).collect();
        assert_eq!(cals.len(), 1);
        assert_eq!(cals[0].displayname, "Work");
        assert_eq!(cals[0].color, "#ff6b6b");
    }
}
