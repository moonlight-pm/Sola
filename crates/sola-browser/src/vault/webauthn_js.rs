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
    s = s.replace(/-/g,'+').replace(/_/g,'/');
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
    return o;
  }
  function toCredential(j){
    var rawId = fromB64url(j.rawId || j.id);
    var id = j.id;
    var clientDataJSON = fromB64url(j.clientDataJSON);
    var authenticatorData = fromB64url(j.authenticatorData);
    var signature = fromB64url(j.signature);
    var userHandle = j.userHandle ? fromB64url(j.userHandle) : null;
    var response = {
      clientDataJSON: clientDataJSON,
      authenticatorData: authenticatorData,
      signature: signature,
      userHandle: userHandle,
      getClientExtensionResults: function(){ return {}; }
    };
    // Some sites read via getters on AuthenticatorAssertionResponse prototype.
    try {
      Object.setPrototypeOf(response, AuthenticatorAssertionResponse.prototype);
    } catch(e) {}
    var cred = {
      id: id,
      rawId: rawId,
      type: 'public-key',
      authenticatorAttachment: 'platform',
      response: response,
      getClientExtensionResults: function(){ return {}; }
    };
    try {
      Object.setPrototypeOf(cred, PublicKeyCredential.prototype);
    } catch(e) {}
    return cred;
  }
  window.__solaWebAuthnResolve = function(id, ok, payload){
    var p = pending[id];
    if (!p) return;
    delete pending[id];
    if (ok) {
      try {
        var j = (typeof payload === 'string') ? JSON.parse(payload) : payload;
        p.resolve(toCredential(j));
      } catch (e) {
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
      // Host timeout → fail soft so the site can fall back.
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
    // Registration not implemented — leave to platform / fail.
    return origCreate ? origCreate(options)
      : Promise.reject(new DOMException('Passkey registration is not supported in Sola yet.', 'NotSupportedError'));
  };
})();"#
}

/// JS that resolves a pending WebAuthn promise (payload is JSON string).
pub fn resolve_webauthn_script(id: u64, ok: bool, payload_json: &str) -> String {
    let payload = serde_json::to_string(payload_json).unwrap_or_else(|_| "\"\"".into());
    format!(
        "(function(){{ try {{ if (window.__solaWebAuthnResolve) window.__solaWebAuthnResolve({id}, {}, {}); }} catch(e) {{ console.error(e); }} }})();",
        if ok { "true" } else { "false" },
        payload
    )
}
