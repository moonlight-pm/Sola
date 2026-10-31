//! Build page-fill scripts for logins, one-time codes, cards and identities.
//!
//! Each script is an IIFE: its values as JSON literals (quotes, backslashes,
//! newlines and `</script>` in a secret cannot break out of the JS string),
//! then the shared `PRELUDE`, then its own body, which leaves its result in
//! `ok`. The browser picks the frames a script runs in; a script fills only
//! visible, enabled, writable fields of the right kind.

/// Helpers every script shares: field visibility, the field text the scorers
/// read, and `setVal`, which fills an input or picks a `<select>` option.
const PRELUDE: &str = r#"  function visible(el){
    if(!el||!el.isConnected) return false;
    var s=window.getComputedStyle(el);
    if(s.display==='none'||s.visibility!=='visible'||parseFloat(s.opacity)<0.1) return false;
    if(el.checkVisibility&&!el.checkVisibility({opacityProperty:true})) return false;
    var r=el.getBoundingClientRect();
    // Honeypots: specks, and fields parked off the page.
    return r.width>1&&r.height>1&&r.right+window.scrollX>0&&r.bottom+window.scrollY>0;
  }
  function usable(el){
    return visible(el)&&!el.disabled&&!el.readOnly;
  }
  function inputs(scope){
    return Array.prototype.slice.call(scope.querySelectorAll(
      'input:not([type="hidden"]):not([type="submit"]):not([type="button"]):not([type="checkbox"]):not([type="radio"]):not([type="password"]):not([type="file"]):not([type="image"])'
    )).filter(usable);
  }
  function blob(el){
    return ((el.getAttribute('autocomplete')||'')+' '+(el.name||'')+' '+(el.id||'')+' '+(el.placeholder||'')+' '+(el.getAttribute('aria-label')||'')+' '+(el.type||'')).toLowerCase();
  }
  function fire(el,type,Ctor,init){
    init=init||{};
    init.bubbles=true;
    try{ el.dispatchEvent(new Ctor(type,init)); }
    catch(e){ el.dispatchEvent(new Event(type,{bubbles:true})); }
  }
  // focus()/blur() fire their own events only while the page has focus (not
  // while the vault panel does): fire synthetic ones only when they did not.
  function focusPair(el,type,pair){
    var hit=false;
    function on(){ hit=true; }
    el.addEventListener(type,on);
    try{ el[type](); }catch(e){}
    el.removeEventListener(type,on);
    if(!hit){ fire(el,type,window.FocusEvent); fire(el,pair,window.FocusEvent); }
  }
  // Index of the option whose text, else value, is one of `wants`, ignoring
  // case. Text first: with zero-based values, value '3' is April, not March.
  function option(el,wants){
    var w=wants.map(function(x){ return String(x).trim().toLowerCase(); });
    for(var byValue=0;byValue<2;byValue++){
      for(var i=0;i<el.options.length;i++){
        var o=el.options[i];
        var t=String(byValue?o.value:o.text).trim().toLowerCase();
        if(t&&w.indexOf(t)>=0) return i;
      }
    }
    return -1;
  }
  // Fill like a typist: React reads the native setter, and forms that
  // validate on keyup or blur see those events.
  function setVal(el,v,wants){
    if(!usable(el)||v===null||v===undefined||v==='') return false;
    var opt=-1;
    if(el.tagName==='SELECT'){
      opt=option(el,wants||[v]);
      if(opt<0) return false;
    }
    focusPair(el,'focus','focusin');
    fire(el,'keydown',window.KeyboardEvent);
    if(opt>=0) el.selectedIndex=opt;
    else{
      var C=el.tagName==='TEXTAREA'?window.HTMLTextAreaElement:window.HTMLInputElement;
      var d=Object.getOwnPropertyDescriptor(C.prototype,'value');
      if(d&&d.set) d.set.call(el,v); else el.value=v;
    }
    fire(el,'input',window.InputEvent,{data:String(v),inputType:'insertText'});
    fire(el,'keyup',window.KeyboardEvent);
    fire(el,'change',window.Event);
    focusPair(el,'blur','focusout');
    return true;
  }
  // Leave the caret in the last field filled so Enter submits the form.
  function rest(el){ try{ if(el) el.focus(); }catch(e){} }
"#;

/// Tells chrome whether the fill found any field.
const REPORT: &str = "  try{ console.info('__sola_vault_fill__:'+(ok?1:0)); }catch(e){}\n";

/// Return an IIFE that fills the most likely username + the current password.
pub fn fill_credentials_script(username: Option<&str>, password: Option<&str>) -> String {
    fill_credentials_script_ex(username, password, false)
}

/// `report` marks the fill after creating a login on a sign-up page: it fills
/// every password field (new + confirm) and `console.info`s
/// `__sola_vault_fill__:1` or `:0` so chrome can tell if any field was found.
/// Without it, only the current password is filled.
pub fn fill_credentials_script_ex(
    username: Option<&str>,
    password: Option<&str>,
    report: bool,
) -> String {
    script(
        &[
            ("user", js_str(username)),
            ("pass", js_str(password)),
            ("create", report.to_string()),
        ],
        LOGIN_JS,
        report,
    )
}

const LOGIN_JS: &str = r#"  function scoreUser(el){
    var a=blob(el);
    var s=0;
    if(/username|email|user|login|account|identifier/.test(a)) s+=5;
    if(el.type==='email') s+=4;
    if(el.type==='text'||el.type==='tel') s+=1;
    if(/password|pass|pwd|search|captcha|otp|code|token|csrf/.test(a)) s-=10;
    return s;
  }
  function hasToken(el,t){
    return (el.getAttribute('autocomplete')||'').toLowerCase().split(/\s+/).indexOf(t)>=0;
  }
  function isNew(el){
    return hasToken(el,'new-password')||/new|confirm|repeat|retype/.test(((el.name||'')+' '+(el.id||'')).toLowerCase());
  }
  var pwds=Array.prototype.slice.call(document.querySelectorAll('input[type="password"]')).filter(usable);
  if(!create){
    // A login takes only the current password: on a change-password form
    // the new and confirm fields stay empty.
    var cur=pwds.filter(function(el){ return hasToken(el,'current-password'); });
    if(cur.length) pwds=cur;
    else if(pwds.length>1) pwds=pwds.filter(function(el){ return !isNew(el); }).slice(0,1);
  }
  var pwd=pwds[0]||null;
  var candidates=inputs(pwd&&pwd.form?pwd.form:document);
  candidates.sort(function(a,b){ return scoreUser(b)-scoreUser(a); });
  // With no password field to anchor on, a bare text box is not a username.
  var floor=pwd?0:3;
  var userEl=candidates.length&&scoreUser(candidates[0])>floor?candidates[0]:null;
  if(user) setVal(userEl,user);
  if(pass){ for(var i=0;i<pwds.length;i++) setVal(pwds[i],pass); }
  rest(pass&&pwds.length?pwds[pwds.length-1]:user?userEl:null);
  var ok=!!(userEl||pwds.length);
"#;

/// IIFE that fills a one-time / authenticator code field.
pub fn fill_totp_script(code: &str) -> String {
    script(&[("code", js_str(Some(code)))], TOTP_JS, false)
}

const TOTP_JS: &str = r#"  var OTP=/otp|totp|2fa|mfa|one.?time|verification|security.?code|auth.?code|token/;
  // Identifiers only: a real code field's label often says "sent to your phone".
  var NOT_OTP=/postal|zip|phone|\btel\b|search|card|cc-|cvv|cvc|email|username|password/;
  function ids(el){
    return ((el.getAttribute('autocomplete')||'')+' '+(el.name||'')+' '+(el.id||'')).toLowerCase();
  }
  function labels(el){
    var t='';
    for(var i=0;el.labels&&i<el.labels.length;i++) t+=' '+el.labels[i].textContent;
    return t.toLowerCase();
  }
  function score(el){
    if(NOT_OTP.test(ids(el))||el.type==='search'||el.type==='email') return 0;
    var s=0;
    if((el.getAttribute('autocomplete')||'').toLowerCase().indexOf('one-time-code')>=0) s+=10;
    if(OTP.test(blob(el)+labels(el))) s+=8;
    // Numeric and 6-8 long fit a postal code too: they only rank OTP fields.
    if(!s) return 0;
    if((el.getAttribute('inputmode')||'').toLowerCase()==='numeric') s+=2;
    if(el.maxLength>=6&&el.maxLength<=8) s+=2;
    return s;
  }
  var els=inputs(document);
  // Split boxes: code.length maxlength=1 inputs under one parent (or one
  // grandparent, when each box has a wrapper) take a character each.
  function boxRun(){
    var boxes=els.filter(function(el){ return el.maxLength===1&&!NOT_OTP.test(ids(el)); });
    for(var up=1;up<=2;up++){
      var groups=new Map(), hit=null;
      boxes.forEach(function(el){
        var p=el;
        for(var i=0;i<up&&p;i++) p=p.parentElement;
        if(!p) return;
        if(!groups.has(p)) groups.set(p,[]);
        groups.get(p).push(el);
      });
      groups.forEach(function(g){ if(!hit&&g.length===code.length) hit=g; });
      if(hit) return hit;
    }
    return null;
  }
  var ok=false;
  var run=code.length>1?boxRun():null;
  if(run){
    for(var i=0;i<run.length;i++) ok=setVal(run[i],code.charAt(i))||ok;
    if(ok) rest(run[Math.min(code.length,run.length)-1]);
  }else{
    els.sort(function(a,b){ return score(b)-score(a); });
    if(els.length&&score(els[0])>0&&setVal(els[0],code)){ ok=true; rest(els[0]); }
  }
"#;

/// IIFE that fills typical checkout card fields (number, name, expiry, CVC).
pub fn fill_card_script(
    cardholder_name: Option<&str>,
    number: Option<&str>,
    exp_month: Option<&str>,
    exp_year: Option<&str>,
    code: Option<&str>,
    brand: Option<&str>,
) -> String {
    script(
        &[
            ("name", js_str(cardholder_name)),
            ("number", js_str(number)),
            ("month", js_str(exp_month)),
            ("year", js_str(exp_year)),
            ("code", js_str(code)),
            ("brand", js_str(brand)),
            ("months", js_list(&month_options(exp_month))),
            ("years", js_list(&year_options(exp_year))),
        ],
        CARD_JS,
        false,
    )
}

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// What a month `<select>` may call the month: `3`, `03`, `March`, `Mar`.
fn month_options(month: Option<&str>) -> Vec<String> {
    let raw = month.unwrap_or("").trim();
    match raw.parse::<usize>() {
        Ok(n @ 1..=12) => {
            let name = MONTHS[n - 1];
            vec![
                n.to_string(),
                format!("{n:02}"),
                name.to_string(),
                name[..3].to_string(),
            ]
        }
        _ if raw.is_empty() => Vec::new(),
        _ => vec![raw.to_string()],
    }
}

/// What a year `<select>` may call the year: `2028` or `28`.
fn year_options(year: Option<&str>) -> Vec<String> {
    let digits: String = year
        .unwrap_or("")
        .chars()
        .filter(char::is_ascii_digit)
        .collect();
    match digits.len() {
        0 => Vec::new(),
        2 => vec![format!("20{digits}"), digits],
        4 => vec![digits.clone(), digits[2..].to_string()],
        _ => vec![digits],
    }
}

const CARD_JS: &str = r#"  function digits(s){ return String(s||'').replace(/\D/g,''); }
  function pad2(s){ s=String(s||''); return s.length===1?('0'+s):s; }
  var month2=pad2(month);
  var yearD=digits(year);
  var year2=yearD.length>=2?yearD.slice(-2):yearD;
  var year4=yearD.length===2?('20'+yearD):yearD;
  var expSlash=month2&&year2?(month2+'/'+year2):'';
  var expSpace=month2&&year2?(month2+' / '+year2):'';
  var expLong=month2&&year4?(month2+'/'+year4):'';
  function score(el, kind){
    var a=blob(el);
    var ac=(el.getAttribute('autocomplete')||'').toLowerCase();
    if(kind==='number'){
      if(ac==='cc-number') return 12;
      if(/cc-number|cardnumber|card-number|card_number|ccnumber|addcreditcardnumber/.test(a)) return 10;
      if(/card.?num|pan\b/.test(a) && !/cvc|cvv|csc|cid/.test(a)) return 6;
      return 0;
    }
    if(kind==='name'){
      if(ac==='cc-name') return 12;
      if(/cc-name|ccname|cardholder|card-holder|nameoncard|name-on-card|name_on_card/.test(a)) return 10;
      if(/holder/.test(a)) return 5;
      return 0;
    }
    if(kind==='exp'){
      if(ac==='cc-exp') return 12;
      if(/cc-exp\b|ccexp|expir|exp-date|exp_date|valid.?thru/.test(a) && !/month|year/.test(a)) return 8;
      return 0;
    }
    if(kind==='month'){
      if(ac==='cc-exp-month') return 12;
      if(/cc-exp-month|exp.?month|expir.*month|month/.test(a) && /exp|cc|card/.test(a)) return 10;
      if(/exp.?mo|ccmonth/.test(a)) return 8;
      return 0;
    }
    if(kind==='year'){
      if(ac==='cc-exp-year') return 12;
      if(/cc-exp-year|exp.?year|expir.*year/.test(a)) return 10;
      if(/ccyear/.test(a)) return 8;
      return 0;
    }
    if(kind==='code'){
      if(ac==='cc-csc') return 12;
      if(/cc-csc|cvc|cvv|csc|cid|security.?code|card.?code/.test(a)) return 10;
      return 0;
    }
    if(kind==='brand'){
      if(ac==='cc-type') return 12;
      if(/cc-type|card.?type|card.?brand/.test(a)) return 8;
      return 0;
    }
    return 0;
  }
  var nodes=Array.prototype.slice.call(document.querySelectorAll('input,select,textarea')).filter(function(el){
    var t=(el.type||'').toLowerCase();
    if(t==='hidden'||t==='submit'||t==='button'||t==='checkbox'||t==='radio'||t==='file'||t==='image') return false;
    return usable(el);
  });
  function best(kind){
    var hit=null, bestS=0;
    for(var i=0;i<nodes.length;i++){
      var s=score(nodes[i], kind);
      if(s>bestS){ bestS=s; hit=nodes[i]; }
    }
    return hit;
  }
  var ok=false;
  ok=setVal(best('number'), number)||ok;
  ok=setVal(best('name'), name)||ok;
  var expEl=best('exp');
  if(expEl){
    var max=parseInt(expEl.getAttribute('maxlength')||'0',10);
    var v=expSlash;
    if(max>=7 && expLong) v=expLong;
    else if((expEl.placeholder||'').indexOf(' / ')>=0 && expSpace) v=expSpace;
    ok=setVal(expEl,v)||ok;
  }
  var monthEl=best('month');
  if(monthEl) ok=setVal(monthEl, month2, months)||ok;
  var yearEl=best('year');
  if(yearEl){
    var ymax=parseInt(yearEl.getAttribute('maxlength')||'0',10);
    var yv=(ymax===2||(yearEl.placeholder||'').indexOf('YY')>=0 && (yearEl.placeholder||'').indexOf('YYYY')<0)?year2:year4;
    ok=setVal(yearEl, yv, years)||ok;
  }
  ok=setVal(best('code'), code)||ok;
  ok=setVal(best('brand'), brand)||ok;
"#;

/// IIFE that fills typical identity / address form fields.
pub fn fill_identity_script(
    title: Option<&str>,
    first_name: Option<&str>,
    middle_name: Option<&str>,
    last_name: Option<&str>,
    address1: Option<&str>,
    address2: Option<&str>,
    address3: Option<&str>,
    city: Option<&str>,
    state: Option<&str>,
    postal_code: Option<&str>,
    country: Option<&str>,
    company: Option<&str>,
    email: Option<&str>,
    phone: Option<&str>,
    ssn: Option<&str>,
    username: Option<&str>,
    passport_number: Option<&str>,
    license_number: Option<&str>,
) -> String {
    let full = {
        let parts: Vec<&str> = [first_name, middle_name, last_name]
            .into_iter()
            .flatten()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        if parts.is_empty() {
            None
        } else {
            Some(parts.join(" "))
        }
    };
    let vals = serde_json::json!({
        "title": title, "first": first_name, "middle": middle_name, "last": last_name,
        "full": full, "address1": address1, "address2": address2, "address3": address3,
        "city": city, "state": state, "postal": postal_code, "country": country,
        "company": company, "email": email, "phone": phone, "ssn": ssn,
        "username": username, "passport": passport_number, "license": license_number,
    });
    script(&[("vals", inert(vals.to_string()))], IDENTITY_JS, false)
}

const IDENTITY_JS: &str = r#"  function score(el, kind){
    var a=blob(el);
    var ac=(el.getAttribute('autocomplete')||'').toLowerCase();
    if(kind==='title'){
      if(ac==='honorific-prefix') return 12;
      if(/honorific-prefix|nameprefix|name-prefix|\btitle\b/.test(a) && !/job|page|post/.test(a)) return 8;
      return 0;
    }
    if(kind==='first'){
      if(ac==='given-name') return 12;
      if(/given-name|firstname|first-name|first_name|\bfname\b/.test(a)) return 10;
      return 0;
    }
    if(kind==='middle'){
      if(ac==='additional-name') return 12;
      if(/additional-name|middlename|middle-name|middle_name|\bmname\b/.test(a)) return 10;
      return 0;
    }
    if(kind==='last'){
      if(ac==='family-name') return 12;
      if(/family-name|lastname|last-name|last_name|surname|\blname\b/.test(a)) return 10;
      return 0;
    }
    if(kind==='full'){
      if(ac==='name') return 10;
      if(/^(name|full.?name|fullname)$/.test(ac)) return 10;
      if(/\bfull.?name\b|\byour.?name\b/.test(a) && !/user|first|last|card|company|org/.test(a)) return 6;
      return 0;
    }
    if(kind==='address1'){
      if(ac==='address-line1'||ac==='street-address') return 12;
      if(/address-line1|street-address|address1|addr1|address_1/.test(a)) return 10;
      if(/\bstreet\b|\baddress\b/.test(a) && !/email|2|3|city|state|zip|postal|country/.test(a)) return 5;
      return 0;
    }
    if(kind==='address2'){
      if(ac==='address-line2') return 12;
      if(/address-line2|address2|addr2|address_2|apt|suite|unit/.test(a)) return 8;
      return 0;
    }
    if(kind==='address3'){
      if(ac==='address-line3') return 12;
      if(/address-line3|address3|addr3/.test(a)) return 8;
      return 0;
    }
    if(kind==='city'){
      if(ac==='address-level2') return 12;
      if(/address-level2|city|town|locality/.test(a)) return 10;
      return 0;
    }
    if(kind==='state'){
      if(ac==='address-level1') return 12;
      if(/address-level1|\bstate\b|province|region/.test(a)) return 10;
      return 0;
    }
    if(kind==='postal'){
      if(ac==='postal-code') return 12;
      if(/postal-code|zipcode|zip-code|zip_code|\bzip\b|\bpostcode\b/.test(a)) return 10;
      return 0;
    }
    if(kind==='country'){
      if(ac==='country'||ac==='country-name') return 12;
      if(/\bcountry\b/.test(a)) return 8;
      return 0;
    }
    if(kind==='company'){
      if(ac==='organization') return 12;
      if(/organization|\bcompany\b|\borg\b/.test(a) && !/email/.test(a)) return 8;
      return 0;
    }
    if(kind==='email'){
      if(ac==='email'||el.type==='email') return 12;
      if(/\bemail\b/.test(a)) return 10;
      return 0;
    }
    if(kind==='phone'){
      if(ac==='tel'||el.type==='tel') return 12;
      if(/\bphone\b|\btel\b|\bmobile\b/.test(a)) return 10;
      return 0;
    }
    if(kind==='ssn'){
      if(/ssn|social.?security/.test(a)) return 12;
      return 0;
    }
    if(kind==='username'){
      if(ac==='username') return 12;
      if(/username|user-name|user_name/.test(a) && !/email/.test(a)) return 8;
      return 0;
    }
    if(kind==='passport'){
      if(/passport/.test(a)) return 12;
      return 0;
    }
    if(kind==='license'){
      if(/license|licence|driver.?lic/.test(a)) return 12;
      return 0;
    }
    return 0;
  }
  var nodes=Array.prototype.slice.call(document.querySelectorAll('input,select,textarea')).filter(function(el){
    var t=(el.type||'').toLowerCase();
    if(t==='hidden'||t==='submit'||t==='button'||t==='checkbox'||t==='radio'||t==='file'||t==='image'||t==='password') return false;
    return usable(el);
  });
  function best(kind){
    var hit=null, bestS=0;
    for(var i=0;i<nodes.length;i++){
      var s=score(nodes[i], kind);
      if(s>bestS){ bestS=s; hit=nodes[i]; }
    }
    return hit;
  }
  var ok=false;
  var keys=['title','first','middle','last','full','address1','address2','address3','city','state','postal','country','company','email','phone','ssn','username','passport','license'];
  for(var k=0;k<keys.length;k++){
    ok=setVal(best(keys[k]), vals[keys[k]])||ok;
  }
"#;

/// Compose an IIFE: `vars` (already JS literals), `PRELUDE`, `body`, then
/// the optional `REPORT` and `return ok`.
fn script(vars: &[(&str, String)], body: &str, report: bool) -> String {
    let mut s = String::from("(function(){\n");
    for (name, literal) in vars {
        s.push_str(&format!("  var {name}={literal};\n"));
    }
    s.push_str(PRELUDE);
    s.push_str(body);
    if report {
        s.push_str(REPORT);
    }
    s.push_str("  return ok;\n})();");
    s
}

fn js_str(s: Option<&str>) -> String {
    let json = serde_json::to_string(s.unwrap_or("")).unwrap_or_else(|_| "\"\"".into());
    inert(json)
}

fn js_list(items: &[String]) -> String {
    let json = serde_json::to_string(items).unwrap_or_else(|_| "[]".into());
    inert(json)
}

/// Escape `<` and the JS line separators in a JSON literal too, so it stays
/// inert even if the script ever lands inside an HTML `<script>`.
fn inert(json: String) -> String {
    json.replace('<', "\\u003c")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card() -> String {
        fill_card_script(
            Some("Ada Lovelace"),
            Some("4111111111111111"),
            Some("3"),
            Some("2028"),
            Some("123"),
            Some("Visa"),
        )
    }

    fn identity(first: &str) -> String {
        fill_identity_script(
            None,
            Some(first),
            None,
            Some("Doe"),
            Some("1 Main"),
            None,
            None,
            Some("Portland"),
            Some("OR"),
            Some("97201"),
            Some("US"),
            None,
            Some("jane@example.com"),
            None,
            None,
            None,
            None,
            None,
        )
    }

    /// The text of `setVal` in the prelude.
    fn set_val() -> &'static str {
        &PRELUDE[PRELUDE.find("function setVal(").unwrap()..]
    }

    #[test]
    fn values_with_quotes_tags_backslashes_and_newlines_are_inert() {
        let nasty = "a'b\"c</script>d\\e\nf";
        let literal = r#""a'b\"c\u003c/script>d\\e\nf""#;
        let scripts = [
            fill_credentials_script(Some(nasty), Some(nasty)),
            fill_totp_script(nasty),
            fill_card_script(Some(nasty), None, None, None, None, None),
            identity(nasty),
        ];
        for s in &scripts {
            assert!(s.contains(literal), "{s}");
            assert!(!s.contains("</script>"));
            assert!(!s.contains("e\nf"));
        }
        assert_eq!(serde_json::from_str::<String>(&js_str(Some(nasty))).unwrap(), nasty);
    }

    #[test]
    fn every_script_composes_the_prelude_once() {
        let scripts = [
            fill_credentials_script(Some("u"), Some("p")),
            fill_totp_script("123456"),
            card(),
            identity("Jane"),
        ];
        for s in &scripts {
            assert_eq!(s.matches(PRELUDE).count(), 1);
            assert_eq!(s.matches("function visible(").count(), 1);
            assert_eq!(s.matches("function setVal(").count(), 1);
            assert_eq!(s.matches("function blob(").count(), 1);
            assert!(s.starts_with("(function(){") && s.ends_with("  return ok;\n})();"));
        }
    }

    #[test]
    fn fields_must_be_visible_enabled_and_writable() {
        assert!(PRELUDE.contains("return visible(el)&&!el.disabled&&!el.readOnly;"));
        assert!(set_val().starts_with("function setVal(el,v,wants){\n    if(!usable(el)"));
        assert!(PRELUDE.contains(")).filter(usable);"));
    }

    #[test]
    fn login_has_no_visibility_blind_fallbacks() {
        let s = fill_credentials_script(Some("u"), Some("p"));
        assert!(s.contains("querySelectorAll('input[type=\"password\"]')).filter(usable);"));
        assert!(!s.contains("document.querySelector("));
        assert!(!s.contains("pwds=[one]"));
        assert!(s.contains("var floor=pwd?0:3;"));
    }

    #[test]
    fn login_fills_only_the_current_password() {
        let s = fill_credentials_script(Some("u"), Some("p"));
        assert!(s.contains("var create=false;"));
        assert!(s.contains("if(!create){"));
        assert!(s.contains("hasToken(el,'current-password')"));
        assert!(s.contains("hasToken(el,'new-password')"));
        assert!(s.contains("pwds=pwds.filter(function(el){ return !isNew(el); }).slice(0,1)"));
        assert!(!s.contains("__sola_vault_fill__"));
    }

    #[test]
    fn create_login_fills_new_and_confirm_and_reports() {
        let s = fill_credentials_script_ex(Some("u"), Some("p"), true);
        assert!(s.contains("var create=true;"));
        assert!(s.contains("for(var i=0;i<pwds.length;i++) setVal(pwds[i],pass);"));
        assert!(s.contains("console.info('__sola_vault_fill__:'+(ok?1:0))"));
        assert!(s.find(REPORT).unwrap() < s.rfind("return ok;").unwrap());
    }

    #[test]
    fn set_val_uses_the_native_setter() {
        assert!(set_val().contains("Object.getOwnPropertyDescriptor(C.prototype,'value')"));
        assert!(set_val().contains("window.HTMLInputElement"));
        assert!(set_val().contains("d.set.call(el,v)"));
    }

    #[test]
    fn set_val_fires_typist_events_in_order() {
        let body = set_val();
        let order = [
            "focusPair(el,'focus','focusin')",
            "'keydown'",
            "d.set.call(el,v)",
            "'input'",
            "'keyup'",
            "'change'",
            "focusPair(el,'blur','focusout')",
        ];
        let at: Vec<usize> = order.iter().map(|n| body.find(n).unwrap()).collect();
        assert!(at.windows(2).all(|w| w[0] < w[1]), "{at:?}");
        assert_eq!(body.matches("'input'").count(), 1);
        assert!(PRELUDE.contains("init.bubbles=true;"));
    }

    #[test]
    fn totp_fills_split_boxes_one_character_each() {
        let s = fill_totp_script("123456");
        assert!(s.contains("el.maxLength===1"));
        assert!(s.contains("g.length===code.length"));
        assert!(s.contains("setVal(run[i],code.charAt(i))"));
    }

    #[test]
    fn totp_never_types_into_the_focused_field() {
        let s = fill_totp_script("123456");
        assert!(!s.contains("activeElement"));
        assert!(s.contains("if(els.length&&score(els[0])>0&&setVal(els[0],code))"));
    }

    #[test]
    fn totp_numeric_length_needs_an_otp_signal() {
        let s = fill_totp_script("123456");
        assert!(s.contains("indexOf('one-time-code')>=0) s+=10;"));
        assert!(s.contains("var OTP=/otp|totp|2fa|mfa|one.?time|verification|"));
        assert!(s.contains("|security.?code|auth.?code|token/;"));
        assert!(s.contains("OTP.test(blob(el)+labels(el))"));
        let gate = s.find("if(!s) return 0;").unwrap();
        assert!(gate < s.find("==='numeric') s+=2;").unwrap());
        assert!(gate < s.find("el.maxLength<=8) s+=2;").unwrap());
    }

    #[test]
    fn totp_rejects_postal_phone_search_and_card_fields() {
        let s = fill_totp_script("123456");
        assert!(s.contains("var NOT_OTP=/postal|zip|phone|\\btel\\b|search|card|"));
        assert!(s.contains("if(NOT_OTP.test(ids(el))||el.type==='search'"));
    }

    #[test]
    fn card_month_select_matches_number_or_name() {
        assert_eq!(month_options(Some("3")), ["3", "03", "March", "Mar"]);
        assert_eq!(month_options(Some("12")), ["12", "12", "December", "Dec"]);
        let s = card();
        assert!(s.contains(r#"var months=["3","03","March","Mar"];"#));
        assert!(s.contains("setVal(monthEl, month2, months)"));
        assert!(PRELUDE.contains("String(x).trim().toLowerCase()"));
    }

    #[test]
    fn card_year_select_matches_four_or_two_digits() {
        assert_eq!(year_options(Some("2028")), ["2028", "28"]);
        assert_eq!(year_options(Some("28")), ["2028", "28"]);
        let s = card();
        assert!(s.contains(r#"var years=["2028","28"];"#));
        assert!(s.contains("setVal(yearEl, yv, years)"));
        assert!(!s.contains("setVal(monthEl, month2, years)"));
        assert!(!s.contains("setVal(yearEl, yv, months)"));
    }

    #[test]
    fn identity_script_maps_autocomplete() {
        let s = identity("Jane");
        assert!(s.contains(r#""first":"Jane""#));
        assert!(s.contains(r#""full":"Jane Doe""#));
        assert!(s.contains("given-name"));
        assert!(s.contains("postal-code"));
        assert!(s.contains("jane@example.com"));
        assert!(s.contains("ok=setVal(best(keys[k]), vals[keys[k]])||ok;"));
    }

    #[test]
    fn login_and_code_fills_leave_the_caret_for_enter() {
        let login = fill_credentials_script(Some("u"), Some("p"));
        assert!(login.contains("rest(pass&&pwds.length?pwds[pwds.length-1]"));
        let totp = fill_totp_script("123456");
        assert!(totp.contains("rest(els[0])"));
    }
}
