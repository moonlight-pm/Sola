//! Injected WebAuthn intercept + response helper for Bitwarden passkeys.

/// Install navigator.credentials.get/create intercept that posts requests to
/// the host via `console.debug("__sola_webauthn__"+JSON)`.
///
/// ArrayBuffers are base64url-encoded for the wire. The host answers with
/// `window.__solaWebAuthnResolve(id, ok, payloadJsonOrError)`.
pub fn inject_webauthn_intercept_script() -> &'static str {
    r#"(function(){
  if (window.__solaWebAuthnInstalled) return;
  window.__solaWebAuthnInstalled = true;
  var pending = Object.create(null);
  var seq = 0;
  function b64url(buf){
    var bytes = buf instanceof ArrayBuffer ? new Uint8Array(buf)
      : (buf && buf.buffer instanceof ArrayBuffer) ? new Uint8Array(buf.buffer, buf.byteOffset, buf.byteLength)
      : null;
    if(!bytes) return null;
    var bin = '';
    for (var i=0;i<bytes.length;i++) bin += String.fromCharCode(bytes[i]);
    var b64 = btoa(bin).replace(/\+/g,'-').replace(/\//g,'_').replace(/=+$/,'');
    return b64;
  }
  function fromB64url(s){
    if(!s) return new ArrayBuffer(0);
    s = String(s).replace(/-/g,'+').replace(/_/g,'/');
    while (s.length % 4) s += '=';
    var bin = atob(s);
    var out = new Uint8Array(bin.length);
    for (var i=0;i<bin.length;i++) out[i] = bin.charCodeAt(i);
    return out.buffer;
  }
  function serializePk(pk){
    if(!pk) return null;
    var o = {};
    for (var k in pk) {
      if (!Object.prototype.hasOwnProperty.call(pk,k)) continue;
      o[k] = pk[k];
    }
    if (pk.challenge) o.challenge = b64url(pk.challenge);
    if (pk.user) {
      o.user = {
        id: b64url(pk.user.id),
        name: pk.user.name,
        displayName: pk.user.displayName
      };
    }
    if (pk.allowCredentials) {
      o.allowCredentials = pk.allowCredentials.map(function(c){
        return {
          type: c.type || 'public-key',
          id: b64url(c.id),
          transports: c.transports
        };
      });
    }
    if (pk.excludeCredentials) {
      o.excludeCredentials = pk.excludeCredentials.map(function(c){
        return {
          type: c.type || 'public-key',
          id: b64url(c.id),
          transports: c.transports
        };
      });
    }
    // extensions often contain ArrayBuffers — drop non-JSON-safe values.
    if (pk.extensions) {
      try { o.extensions = JSON.parse(JSON.stringify(pk.extensions)); }
      catch (e) { delete o.extensions; }
    }
    return o;
  }
  function toCredential(j){
    var rawIdBuf = fromB64url(j.rawId || j.id);
    // WebAuthn: id is base64url(rawId) as DOMString.
    var id = j.id || b64url(rawIdBuf);
    // Accept both WebAuthn spelling (clientDataJSON) and accidental camelCase.
    var cdB64 = j.clientDataJSON || j.clientDataJson || '';
    var clientDataJSON = fromB64url(cdB64);
    if (!cdB64 || clientDataJSON.byteLength === 0) {
      throw new Error('sola webauthn: missing clientDataJSON in host payload keys=' + Object.keys(j).join(','));
    }

    // Never construct native WebAuthn objects. Chromium's constructors
    // either throw or produce a platform object whose methods reject
    // with "Invalid invocation" when `this` is not a real credential.
    // SimpleWebAuthn / Outline call getTransports / toJSON / getPublicKey.
    var isCreate = !!(j.attestationObject);
    var response;
    var transports = ['internal'];
    var pkAlg = -7;
    var publicKey = null;
    var authenticatorData;
    var attestationObject = null;
    var signature = null;
    var userHandle = null;
    if (isCreate) {
      attestationObject = fromB64url(j.attestationObject);
      authenticatorData = fromB64url(j.authenticatorData || '');
      publicKey = (j.publicKey && String(j.publicKey).length) ? fromB64url(j.publicKey) : null;
      pkAlg = (typeof j.publicKeyAlgorithm === 'number') ? j.publicKeyAlgorithm : -7;
      transports = (Array.isArray(j.transports) && j.transports.length) ? j.transports.slice() : ['internal'];
      response = {
        clientDataJSON: clientDataJSON,
        attestationObject: attestationObject,
        getAuthenticatorData: function(){ return authenticatorData; },
        getPublicKey: function(){ return publicKey; },
        getPublicKeyAlgorithm: function(){ return pkAlg; },
        getTransports: function(){ return transports.slice(); }
      };
      try { Object.setPrototypeOf(response, AuthenticatorAttestationResponse.prototype); } catch (e) {}
    } else {
      var adB64 = j.authenticatorData || j.authenticator_data || '';
      var sigB64 = j.signature || '';
      var uhB64 = j.userHandle || j.user_handle || '';
      authenticatorData = fromB64url(adB64);
      signature = fromB64url(sigB64);
      userHandle = (uhB64 && uhB64.length) ? fromB64url(uhB64) : null;
      response = {
        clientDataJSON: clientDataJSON,
        authenticatorData: authenticatorData,
        signature: signature,
        userHandle: userHandle
      };
      try { Object.setPrototypeOf(response, AuthenticatorAssertionResponse.prototype); } catch (e) {}
    }

    var extResults = {};
    function toJSON(){
      var out = {
        id: id,
        rawId: b64url(rawIdBuf),
        type: 'public-key',
        authenticatorAttachment: 'platform',
        clientExtensionResults: extResults,
        response: isCreate ? {
          clientDataJSON: b64url(clientDataJSON),
          attestationObject: b64url(attestationObject),
          authenticatorData: b64url(authenticatorData),
          publicKey: publicKey ? b64url(publicKey) : null,
          publicKeyAlgorithm: pkAlg,
          transports: transports.slice()
        } : {
          clientDataJSON: b64url(clientDataJSON),
          authenticatorData: b64url(authenticatorData),
          signature: b64url(signature),
          userHandle: userHandle ? b64url(userHandle) : null
        }
      };
      return out;
    }
    var cred = {
      id: id,
      rawId: rawIdBuf,
      type: 'public-key',
      authenticatorAttachment: 'platform',
      response: response,
      getClientExtensionResults: function(){ return extResults; },
      toJSON: toJSON
    };
    try { Object.setPrototypeOf(cred, PublicKeyCredential.prototype); } catch (e) {}
    // Debug breadcrumb for dogfood (host may collect via console handler).
    try {
      var cd = new TextDecoder().decode(clientDataJSON);
      console.debug('__sola_webauthn_cred__', JSON.stringify({
        idLen: id.length,
        rawIdLen: rawIdBuf.byteLength,
        clientData: cd,
        kind: isCreate ? 'create' : 'get'
      }));
    } catch (e) {}
    return cred;
  }
  window.__solaWebAuthnResolve = function(id, ok, payload){
    var p = pending[id];
    if (!p) {
      console.debug('__sola_webauthn_orphan_resolve__', id, ok);
      return;
    }
    delete pending[id];
    if (ok) {
      try {
        var j = (typeof payload === 'string') ? JSON.parse(payload) : payload;
        p.resolve(toCredential(j));
      } catch (e) {
        try { console.error('__sola_webauthn_resolve_err__' + String(e && e.message || e)); } catch (e2) {}
        p.reject(e);
      }
    } else {
      p.reject(new DOMException(String(payload || 'NotAllowedError'), 'NotAllowedError'));
    }
  };
  function post(req){
    // One channel. Gemini Exchange stubs console.debug; log still
    // reaches CEF. Extra console levels + a hidden iframe beacon used
    // to deliver the same click 4× — chrome rejected the live promise
    // as "Superseded" and the page showed "passkey auth failed"
    // before the user could pick.
    try { console.log('__sola_webauthn__' + JSON.stringify(req)); } catch (e) {}
  }
  try { console.info('__sola_webauthn_installed__' + (location && location.origin || '')); } catch (e) {}
  var orig = navigator.credentials && navigator.credentials.get
    ? navigator.credentials.get.bind(navigator.credentials) : null;
  var origCreate = navigator.credentials && navigator.credentials.create
    ? navigator.credentials.create.bind(navigator.credentials) : null;
  if (!navigator.credentials) return;
  navigator.credentials.get = function(options){
    if (!options || !options.publicKey) {
      return orig ? orig(options) : Promise.reject(new DOMException('NotSupportedError'));
    }
    return new Promise(function(resolve, reject){
      var id = ++seq;
      pending[id] = { resolve: resolve, reject: reject };
      var pk = serializePk(options.publicKey);
      post({
        v: 1,
        id: id,
        action: 'get',
        origin: location.origin,
        rpId: (options.publicKey.rpId) || location.hostname,
        publicKey: pk
      });
      setTimeout(function(){
        if (pending[id]) {
          delete pending[id];
          reject(new DOMException('The operation either timed out or was not allowed.', 'NotAllowedError'));
        }
      }, 120000);
    });
  };
  navigator.credentials.create = function(options){
    if (!options || !options.publicKey) {
      return origCreate ? origCreate(options) : Promise.reject(new DOMException('NotSupportedError'));
    }
    // Never fall through to Chromium's native WebAuthn window (OSR cannot
    // host that dialog). Chrome confirms, then the vault registers.
    return new Promise(function(resolve, reject){
      var id = ++seq;
      pending[id] = { resolve: resolve, reject: reject };
      var pk = serializePk(options.publicKey);
      post({
        v: 1,
        id: id,
        action: 'create',
        origin: location.origin,
        rpId: (options.publicKey.rp && options.publicKey.rp.id) || location.hostname,
        publicKey: pk
      });
      setTimeout(function(){
        if (pending[id]) {
          delete pending[id];
          reject(new DOMException('The operation either timed out or was not allowed.', 'NotAllowedError'));
        }
      }, 120000);
    });
  };
  // WebAuthn L3 static methods bypass navigator.credentials in Chromium.
  try {
    if (window.PublicKeyCredential) {
      if (typeof PublicKeyCredential.get === 'function') {
        PublicKeyCredential.get = function(options){
          return navigator.credentials.get(options);
        };
      }
      if (typeof PublicKeyCredential.create === 'function') {
        PublicKeyCredential.create = function(options){
          return navigator.credentials.create(options);
        };
      }
    }
  } catch (e) {}
})();"#
}

/// JS that resolves a pending WebAuthn promise (payload is JSON string).
pub fn resolve_webauthn_script(id: u64, ok: bool, payload_json: &str) -> String {
    resolve_webauthn_scripts(&[id], ok, payload_json)
}

/// Resolve every in-flight page id for one ceremony (duplicates / retries).
pub fn resolve_webauthn_scripts(ids: &[u64], ok: bool, payload_json: &str) -> String {
    let payload = serde_json::to_string(payload_json).unwrap_or_else(|_| "\"\"".into());
    let ok_js = if ok { "true" } else { "false" };
    let calls: String = ids
        .iter()
        .map(|id| format!("window.__solaWebAuthnResolve({id}, {ok_js}, {payload});"))
        .collect();
    format!(
        "(function(){{ try {{ if (window.__solaWebAuthnResolve) {{ {calls} }} }} catch(e) {{ console.error('sola webauthn resolve', e); }} }})();"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_public_key_does_not_fall_through_to_chromium() {
        let s = inject_webauthn_intercept_script();
        let create = s
            .split("navigator.credentials.create = function")
            .nth(1)
            .expect("create hook");
        assert!(
            create.contains("action: 'create'"),
            "publicKey create() must post to chrome"
        );
        assert!(
            create.contains("publicKey: pk"),
            "create() must send serialized publicKey (user.id / challenge)"
        );
        assert!(
            !create.contains("Passkey registration is not supported"),
            "create() must hold the page promise, not reject immediately"
        );
        // origCreate is only the non-publicKey fallback.
        assert_eq!(
            create.matches("origCreate(").count(),
            1,
            "origCreate only for non-publicKey options"
        );
        let pk_path = create
            .split("return new Promise")
            .nth(1)
            .expect("create() holds a promise");
        assert!(
            !pk_path.contains("origCreate("),
            "publicKey create() must not fall through to Chromium"
        );
    }

    #[test]
    fn serialize_encodes_user_id() {
        let s = inject_webauthn_intercept_script();
        assert!(
            s.contains("o.user = {"),
            "create options must encode user.id as base64url"
        );
        assert!(s.contains("id: b64url(pk.user.id)"));
    }

    #[test]
    fn to_credential_handles_attestation() {
        let s = inject_webauthn_intercept_script();
        assert!(s.contains("j.attestationObject"));
        assert!(s.contains("AuthenticatorAttestationResponse"));
        assert!(s.contains("getTransports: function"));
        assert!(s.contains("getPublicKeyAlgorithm: function"));
        assert!(s.contains("toJSON: toJSON"));
        assert!(
            !s.contains("new AuthenticatorAttestationResponse"),
            "native constructors throw Invalid invocation on method calls"
        );
        assert!(
            !s.contains("new PublicKeyCredential"),
            "native PublicKeyCredential constructor is not used"
        );
    }

    #[test]
    fn post_emits_once_via_console_log() {
        let s = inject_webauthn_intercept_script();
        let post = s.split("function post(req)").nth(1).expect("post()");
        let body = post.split("try { console.info").next().unwrap_or(post);
        assert!(
            body.contains("console.log('__sola_webauthn__'"),
            "post() must use console.log (Gemini stubs debug)"
        );
        assert!(
            !body.contains("console.info('__sola_webauthn__'"),
            "do not also post via console.info"
        );
        assert!(
            !body.contains("console.warn('__sola_webauthn__'"),
            "do not also post via console.warn"
        );
        assert!(
            !body.contains("sola.invalid") && !body.contains("createElement('iframe')"),
            "do not also post via a navigation beacon"
        );
        assert_eq!(
            body.matches("console.log('__sola_webauthn__'").count(),
            1,
            "exactly one console.log of the request"
        );
    }

    #[test]
    fn resolve_scripts_fans_out_ids() {
        let js = resolve_webauthn_scripts(&[1, 2], false, "Superseded");
        assert!(js.contains("__solaWebAuthnResolve(1, false"));
        assert!(js.contains("__solaWebAuthnResolve(2, false"));
    }
}
