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
    var clientDataJSON = fromB64url(j.clientDataJSON);
    var authenticatorData = fromB64url(j.authenticatorData);
    var signature = fromB64url(j.signature);
    var userHandle = (j.userHandle && j.userHandle.length) ? fromB64url(j.userHandle) : null;

    // Prefer real WebAuthn response object when the platform allows it.
    var response;
    try {
      response = new AuthenticatorAssertionResponse({
        clientDataJSON: clientDataJSON,
        authenticatorData: authenticatorData,
        signature: signature,
        userHandle: userHandle
      });
    } catch (e) {
      response = {
        clientDataJSON: clientDataJSON,
        authenticatorData: authenticatorData,
        signature: signature,
        userHandle: userHandle
      };
      try {
        Object.setPrototypeOf(response, AuthenticatorAssertionResponse.prototype);
      } catch (e2) {}
    }

    var cred;
    try {
      cred = new PublicKeyCredential({
        id: id,
        rawId: rawIdBuf,
        response: response,
        authenticatorAttachment: 'platform',
        clientExtensionResults: {},
        type: 'public-key'
      });
    } catch (e) {
      cred = {
        id: id,
        rawId: rawIdBuf,
        type: 'public-key',
        authenticatorAttachment: 'platform',
        response: response,
        getClientExtensionResults: function(){ return {}; }
      };
      try {
        Object.setPrototypeOf(cred, PublicKeyCredential.prototype);
      } catch (e2) {}
    }
    if (typeof cred.getClientExtensionResults !== 'function') {
      cred.getClientExtensionResults = function(){ return {}; };
    }
    // Debug breadcrumb for dogfood (host may collect via console handler).
    try {
      var cd = new TextDecoder().decode(clientDataJSON);
      console.debug('__sola_webauthn_cred__', JSON.stringify({
        idLen: id.length,
        rawIdLen: rawIdBuf.byteLength,
        clientData: cd,
        sigLen: signature.byteLength,
        authDataLen: authenticatorData.byteLength
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
        console.error('__sola_webauthn_resolve_err__', e);
        p.reject(e);
      }
    } else {
      p.reject(new DOMException(String(payload || 'NotAllowedError'), 'NotAllowedError'));
    }
  };
  function post(req){
    try {
      console.debug('__sola_webauthn__' + JSON.stringify(req));
    } catch (e) {
      console.log('__sola_webauthn__' + JSON.stringify(req));
    }
  }
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
    return origCreate ? origCreate(options)
      : Promise.reject(new DOMException('Passkey registration is not supported in Sola yet.', 'NotSupportedError'));
  };
})();"#
}

/// JS that resolves a pending WebAuthn promise (payload is JSON string).
pub fn resolve_webauthn_script(id: u64, ok: bool, payload_json: &str) -> String {
    let payload = serde_json::to_string(payload_json).unwrap_or_else(|_| "\"\"".into());
    format!(
        "(function(){{ try {{ if (window.__solaWebAuthnResolve) window.__solaWebAuthnResolve({id}, {}, {}); }} catch(e) {{ console.error('sola webauthn resolve', e); }} }})();",
        if ok { "true" } else { "false" },
        payload
    )
}
