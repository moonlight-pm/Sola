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
    fn fills_every_password_and_can_report() {
        let s = fill_credentials_script_ex(Some("u"), Some("p"), true);
        assert!(s.contains("for(var i=0;i<pwds.length;i++)"));
        assert!(s.contains("__sola_vault_fill__"));
        let quiet = fill_credentials_script(Some("u"), Some("p"));
        assert!(!quiet.contains("__sola_vault_fill__"));
    }
}
