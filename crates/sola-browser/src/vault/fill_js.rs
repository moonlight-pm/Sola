//! Build a page-fill script that sets username/password on login fields.
//!
//! Values are embedded as JSON string literals so quotes / newlines / `</script>`
//! in the secret cannot break out of the JS string.

/// Return an IIFE that fills the most likely username + every visible password.
///
/// When `report` is true, the script also `console.info`s
/// `__sola_vault_fill__:1` or `:0` so chrome can tell if any field was found.
pub fn fill_credentials_script(username: Option<&str>, password: Option<&str>) -> String {
    fill_credentials_script_ex(username, password, false)
}

pub fn fill_credentials_script_ex(
    username: Option<&str>,
    password: Option<&str>,
    report: bool,
) -> String {
    let user = serde_json::to_string(username.unwrap_or("")).unwrap_or_else(|_| "\"\"".into());
    let pass = serde_json::to_string(password.unwrap_or("")).unwrap_or_else(|_| "\"\"".into());
    let report_js = if report {
        "try{ console.info('__sola_vault_fill__:'+(ok?1:0)); }catch(e){}"
    } else {
        ""
    };

    // Keep this self-contained: no external deps, works on typical login forms.
    // Dispatches input/change so React/Vue-style controlled inputs update.
    format!(
        r#"(function(){{
  var user={user};
  var pass={pass};
  function visible(el){{
    if(!el) return false;
    var s=window.getComputedStyle(el);
    if(s.display==='none'||s.visibility==='hidden'||s.opacity==='0') return false;
    var r=el.getBoundingClientRect();
    return r.width>0 && r.height>0;
  }}
  function setVal(el,v){{
    if(!el||v===null||v===undefined||v==='') return;
    try{{ el.focus(); }}catch(e){{}}
    var proto=window.HTMLInputElement&&window.HTMLInputElement.prototype;
    var desc=proto&&Object.getOwnPropertyDescriptor(proto,'value');
    if(desc&&desc.set){{ desc.set.call(el,v); }}
    else {{ el.value=v; }}
    el.dispatchEvent(new Event('input',{{bubbles:true}}));
    el.dispatchEvent(new Event('change',{{bubbles:true}}));
    try{{ el.dispatchEvent(new InputEvent('input',{{bubbles:true,data:v,inputType:'insertText'}})); }}catch(e){{}}
  }}
  function scoreUser(el){{
    var a=((el.getAttribute('autocomplete')||'')+' '+(el.name||'')+' '+(el.id||'')+' '+(el.type||'')+' '+(el.placeholder||'')).toLowerCase();
    var s=0;
    if(/username|email|user|login|account|identifier/.test(a)) s+=5;
    if(el.type==='email') s+=4;
    if(el.type==='text'||el.type==='tel') s+=1;
    if(/password|pass|pwd|search|captcha|otp|code|token|csrf/.test(a)) s-=10;
    return s;
  }}
  var pwds=Array.prototype.slice.call(document.querySelectorAll('input[type="password"]')).filter(visible);
  if(!pwds.length){{
    var one=document.querySelector('input[type="password"]');
    if(one) pwds=[one];
  }}
  var pwd=pwds[0]||null;
  var userEl=null;
  var scope=pwd&&pwd.form?pwd.form:document;
  var candidates=Array.prototype.slice.call(scope.querySelectorAll(
    'input:not([type="hidden"]):not([type="submit"]):not([type="button"]):not([type="checkbox"]):not([type="radio"]):not([type="password"]):not([type="file"]):not([type="image"])'
  )).filter(visible);
  candidates.sort(function(a,b){{ return scoreUser(b)-scoreUser(a); }});
  if(candidates.length&&scoreUser(candidates[0])>0) userEl=candidates[0];
  if(!userEl){{
    userEl=document.querySelector('input[autocomplete="username"],input[type="email"],input[name*="user" i],input[name*="email" i],input[id*="user" i],input[id*="email" i]');
  }}
  if(user) setVal(userEl,user);
  if(pass){{ for(var i=0;i<pwds.length;i++) setVal(pwds[i],pass); }}
  var ok=!!(userEl||pwds.length);
  {report_js}
  return ok;
}})();"#
    )
}

/// IIFE that fills a one-time / authenticator code field.
pub fn fill_totp_script(code: &str) -> String {
    let code = serde_json::to_string(code).unwrap_or_else(|_| "\"\"".into());
    format!(
        r#"(function(){{
  var code={code};
  function visible(el){{
    if(!el) return false;
    var s=window.getComputedStyle(el);
    if(s.display==='none'||s.visibility==='hidden'||s.opacity==='0') return false;
    var r=el.getBoundingClientRect();
    return r.width>0 && r.height>0;
  }}
  function setVal(el,v){{
    if(!el||!v) return;
    try{{ el.focus(); }}catch(e){{}}
    var proto=window.HTMLInputElement&&window.HTMLInputElement.prototype;
    var desc=proto&&Object.getOwnPropertyDescriptor(proto,'value');
    if(desc&&desc.set) desc.set.call(el,v); else el.value=v;
    el.dispatchEvent(new Event('input',{{bubbles:true}}));
    el.dispatchEvent(new Event('change',{{bubbles:true}}));
    try{{ el.dispatchEvent(new InputEvent('input',{{bubbles:true,data:v,inputType:'insertText'}})); }}catch(e){{}}
  }}
  function score(el){{
    var a=((el.getAttribute('autocomplete')||'')+' '+(el.name||'')+' '+(el.id||'')+' '+(el.placeholder||'')+' '+(el.getAttribute('inputmode')||'')+' '+(el.getAttribute('aria-label')||'')).toLowerCase();
    var s=0;
    if(/one-time-code|one.time|otp|totp|2fa|mfa|authenticator|verification.?code|security.?code/.test(a)) s+=8;
    if((el.maxLength===6||el.maxLength===7||el.maxLength===8)&&el.type!=='password') s+=3;
    if(el.inputMode==='numeric'||el.getAttribute('inputmode')==='numeric') s+=2;
    if(/password|search|email|username|card|cvv|cvc/.test(a)) s-=8;
    return s;
  }}
  var els=Array.prototype.slice.call(document.querySelectorAll(
    'input:not([type="hidden"]):not([type="submit"]):not([type="button"]):not([type="checkbox"]):not([type="radio"]):not([type="password"]):not([type="file"])'
  )).filter(visible);
  els.sort(function(a,b){{ return score(b)-score(a); }});
  var el=els.length&&score(els[0])>0?els[0]:null;
  if(!el){{
    var ae=document.activeElement;
    if(ae&&ae.tagName==='INPUT'&&visible(ae)&&ae.type!=='password') el=ae;
  }}
  if(el) setVal(el,code);
  return !!el;
}})();"#
    )
}

/// IIFE that fills typical checkout card fields (number, name, expiry, CVC).
pub fn fill_card_script(
    cardholder_name: Option<&str>,
    number: Option<&str>,
    exp_month: Option<&str>,
    exp_year: Option<&str>,
    code: Option<&str>,
    brand: Option<&str>,
) -> String {
    let name = js_str(cardholder_name);
    let number = js_str(number);
    let month = js_str(exp_month);
    let year = js_str(exp_year);
    let code = js_str(code);
    let brand = js_str(brand);

    format!(
        r#"(function(){{
  var name={name};
  var number={number};
  var month={month};
  var year={year};
  var code={code};
  var brand={brand};
  function digits(s){{ return String(s||'').replace(/\D/g,''); }}
  function pad2(s){{ s=String(s||''); return s.length===1?('0'+s):s; }}
  var month2=pad2(month);
  var yearD=digits(year);
  var year2=yearD.length>=2?yearD.slice(-2):yearD;
  var year4=yearD.length===2?('20'+yearD):yearD;
  var expSlash=month2&&year2?(month2+'/'+year2):'';
  var expSpace=month2&&year2?(month2+' / '+year2):'';
  var expLong=month2&&year4?(month2+'/'+year4):'';
  function visible(el){{
    if(!el) return false;
    var s=window.getComputedStyle(el);
    if(s.display==='none'||s.visibility==='hidden'||s.opacity==='0') return false;
    var r=el.getBoundingClientRect();
    return r.width>0 && r.height>0;
  }}
  function blob(el){{
    return ((el.getAttribute('autocomplete')||'')+' '+(el.name||'')+' '+(el.id||'')+' '+(el.placeholder||'')+' '+(el.getAttribute('aria-label')||'')+' '+(el.type||'')).toLowerCase();
  }}
  function setVal(el,v){{
    if(!el||v===null||v===undefined||v==='') return false;
    try{{ el.focus(); }}catch(e){{}}
    if(el.tagName==='SELECT'){{
      var want=[String(v), pad2(v), digits(v), year2, year4];
      var opts=el.options||[];
      for(var i=0;i<opts.length;i++){{
        var ov=String(opts[i].value||'');
        var ot=String(opts[i].text||'');
        for(var k=0;k<want.length;k++){{
          if(!want[k]) continue;
          if(ov===want[k]||ot===want[k]||ov.endsWith(want[k])||digits(ov)===digits(want[k])){{
            el.selectedIndex=i;
            el.dispatchEvent(new Event('input',{{bubbles:true}}));
            el.dispatchEvent(new Event('change',{{bubbles:true}}));
            return true;
          }}
        }}
      }}
      return false;
    }}
    var proto=(el.tagName==='TEXTAREA'?window.HTMLTextAreaElement:window.HTMLInputElement);
    proto=proto&&proto.prototype;
    var desc=proto&&Object.getOwnPropertyDescriptor(proto,'value');
    if(desc&&desc.set){{ desc.set.call(el,v); }}
    else {{ el.value=v; }}
    el.dispatchEvent(new Event('input',{{bubbles:true}}));
    el.dispatchEvent(new Event('change',{{bubbles:true}}));
    try{{ el.dispatchEvent(new InputEvent('input',{{bubbles:true,data:v,inputType:'insertText'}})); }}catch(e){{}}
    return true;
  }}
  function score(el, kind){{
    var a=blob(el);
    var ac=(el.getAttribute('autocomplete')||'').toLowerCase();
    if(kind==='number'){{
      if(ac==='cc-number') return 12;
      if(/cc-number|cardnumber|card-number|card_number|ccnumber|addcreditcardnumber/.test(a)) return 10;
      if(/card.?num|pan\b/.test(a) && !/cvc|cvv|csc|cid/.test(a)) return 6;
      return 0;
    }}
    if(kind==='name'){{
      if(ac==='cc-name') return 12;
      if(/cc-name|ccname|cardholder|card-holder|nameoncard|name-on-card|name_on_card/.test(a)) return 10;
      if(/holder/.test(a)) return 5;
      return 0;
    }}
    if(kind==='exp'){{
      if(ac==='cc-exp') return 12;
      if(/cc-exp\b|ccexp|expir|exp-date|exp_date|valid.?thru/.test(a) && !/month|year/.test(a)) return 8;
      return 0;
    }}
    if(kind==='month'){{
      if(ac==='cc-exp-month') return 12;
      if(/cc-exp-month|exp.?month|expir.*month|month/.test(a) && /exp|cc|card/.test(a)) return 10;
      if(/exp.?mo|ccmonth/.test(a)) return 8;
      return 0;
    }}
    if(kind==='year'){{
      if(ac==='cc-exp-year') return 12;
      if(/cc-exp-year|exp.?year|expir.*year/.test(a)) return 10;
      if(/ccyear/.test(a)) return 8;
      return 0;
    }}
    if(kind==='code'){{
      if(ac==='cc-csc') return 12;
      if(/cc-csc|cvc|cvv|csc|cid|security.?code|card.?code/.test(a)) return 10;
      return 0;
    }}
    if(kind==='brand'){{
      if(ac==='cc-type') return 12;
      if(/cc-type|card.?type|card.?brand/.test(a)) return 8;
      return 0;
    }}
    return 0;
  }}
  var nodes=Array.prototype.slice.call(document.querySelectorAll('input,select,textarea')).filter(function(el){{
    if(el.disabled||el.readOnly) return false;
    var t=(el.type||'').toLowerCase();
    if(t==='hidden'||t==='submit'||t==='button'||t==='checkbox'||t==='radio'||t==='file'||t==='image') return false;
    return visible(el);
  }});
  function best(kind){{
    var hit=null, bestS=0;
    for(var i=0;i<nodes.length;i++){{
      var s=score(nodes[i], kind);
      if(s>bestS){{ bestS=s; hit=nodes[i]; }}
    }}
    return hit;
  }}
  var ok=false;
  ok=setVal(best('number'), number)||ok;
  ok=setVal(best('name'), name)||ok;
  var expEl=best('exp');
  if(expEl){{
    var max=parseInt(expEl.getAttribute('maxlength')||'0',10);
    var v=expSlash;
    if(max>=7 && expLong) v=expLong;
    else if((expEl.placeholder||'').indexOf(' / ')>=0 && expSpace) v=expSpace;
    ok=setVal(expEl,v)||ok;
  }}
  var monthEl=best('month');
  if(monthEl) ok=setVal(monthEl, month2)||ok;
  var yearEl=best('year');
  if(yearEl){{
    var ymax=parseInt(yearEl.getAttribute('maxlength')||'0',10);
    var yv=(ymax===2||(yearEl.placeholder||'').indexOf('YY')>=0 && (yearEl.placeholder||'').indexOf('YYYY')<0)?year2:year4;
    if(yearEl.tagName==='SELECT') yv=year4;
    ok=setVal(yearEl, yv)||ok;
  }}
  ok=setVal(best('code'), code)||ok;
  ok=setVal(best('brand'), brand)||ok;
  return ok;
}})();"#
    )
}

fn js_str(s: Option<&str>) -> String {
    serde_json::to_string(s.unwrap_or("")).unwrap_or_else(|_| "\"\"".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embeds_quotes_safely() {
        let s = fill_credentials_script(Some(r#"a"b"#), Some("p'ass"));
        assert!(s.contains(r#"a\"b"#) || s.contains("a\\\"b"));
        assert!(s.contains("p'ass") || s.contains("p\\'ass") || s.contains("\"p'ass\""));
        assert!(!s.contains("\0"));
    }

    #[test]
    fn totp_script_scores_otp_fields() {
        let s = fill_totp_script("123456");
        assert!(s.contains("123456"));
        assert!(s.contains("one-time-code"));
        assert!(s.contains("totp"));
    }

    #[test]
    fn fills_every_password_and_can_report() {
        let s = fill_credentials_script_ex(Some("u"), Some("p"), true);
        assert!(s.contains("for(var i=0;i<pwds.length;i++)"));
        assert!(s.contains("__sola_vault_fill__"));
        let quiet = fill_credentials_script(Some("u"), Some("p"));
        assert!(!quiet.contains("__sola_vault_fill__"));
    }

    #[test]
    fn card_script_embeds_fields() {
        let s = fill_card_script(
            Some("Ada Lovelace"),
            Some("4111111111111111"),
            Some("12"),
            Some("2028"),
            Some("123"),
            Some("Visa"),
        );
        assert!(s.contains("4111111111111111"));
        assert!(s.contains("Ada Lovelace"));
        assert!(s.contains("cc-number") || s.contains("cardnumber"));
        assert!(s.contains("setVal"));
        assert!(!s.contains("\0"));
    }
}
