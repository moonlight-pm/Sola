//! Injected WebAuthn intercept + response helper for Bitwarden passkeys.

/// Install navigator.credentials.get/create intercept that posts requests to
/// the host through `window.__solaVaultPost(json)`, a native function the
/// renderer installs first: it reaches the helper as a message from this
/// frame, so the helper takes the origin from CEF, not from this JSON.
/// Injected at V8 context creation so the page cannot snapshot native `get`
/// before the hook lands.
///
/// Each request has a random `nonce`. ArrayBuffers are base64url-encoded
/// for the wire. The host answers in the requesting frame only, with
/// `window.__solaWebAuthnResolve(nonce, ok, payloadJsonOrError)`. When the
/// page stops waiting it posts `{action:'cancel', nonce}`.
pub fn inject_webauthn_intercept_script() -> &'static str {
    r#"(function(){
  if (window.__solaWebAuthnInstalled) return;
  window.__solaWebAuthnInstalled = true;
  var nativePost = window.__solaVaultPost;
  if (typeof nativePost !== 'function') return;
  var pending = Object.create(null);
  function nonce(){
    var b = new Uint8Array(16);
    crypto.getRandomValues(b);
    var s = '';
    for (var i=0;i<b.length;i++) s += (b[i] < 16 ? '0' : '') + b[i].toString(16);
    return s;
  }
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
        try { console.error('__sola_webauthn_resolve_err__' + String(e && e.message || e)); } catch (e2) {}
        p.reject(e);
      }
    } else {
      p.reject(new DOMException(String(payload || 'NotAllowedError'), 'NotAllowedError'));
    }
  };
  function post(req){
    try { nativePost(JSON.stringify(req)); } catch (e) {}
  }
  // Hold the page's promise until chrome answers; on the page's own
  // timeout, reject and tell chrome to drop the request.
  function ask(action, rpId, publicKey){
    return new Promise(function(resolve, reject){
      var id = nonce();
      pending[id] = { resolve: resolve, reject: reject };
      post({ v: 2, nonce: id, action: action, rpId: rpId, publicKey: serializePk(publicKey) });
      var ms = Math.min(Math.max(Number(publicKey.timeout) || 120000, 30000), 600000);
      setTimeout(function(){
        if (pending[id]) {
          delete pending[id];
          post({ v: 2, nonce: id, action: 'cancel' });
          reject(new DOMException('The operation either timed out or was not allowed.', 'NotAllowedError'));
        }
      }, ms);
    });
  }
  if (!navigator.credentials) return;
  var proto = null;
  try { proto = CredentialsContainer.prototype; } catch (e) {}
  var origGet = proto && proto.get ? proto.get : navigator.credentials.get;
  var origCreate = proto && proto.create ? proto.create : navigator.credentials.create;
  function hookedGet(options){
    if (!options || !options.publicKey) {
      return origGet ? origGet.call(navigator.credentials, options)
        : Promise.reject(new DOMException('NotSupportedError', 'NotSupportedError'));
    }
    // OSR has no Chromium passkey sheet. Immediate/conditional get()
    // would otherwise hang on "Verifying it's you" (Google). Ignore
    // AbortSignal — Google aborts immediate get when the native UI
    // finds no platform credential.
    return ask('get', options.publicKey.rpId || location.hostname, options.publicKey);
  }
  function hookedCreate(options){
    if (!options || !options.publicKey) {
      return origCreate ? origCreate.call(navigator.credentials, options)
        : Promise.reject(new DOMException('NotSupportedError', 'NotSupportedError'));
    }
    var rp = options.publicKey.rp;
    return ask('create', (rp && rp.id) || location.hostname, options.publicKey);
  }
  function installMethod(obj, name, fn){
    // Getter lock: assignment like `credentials.get = nativeGet` is
    // swallowed so a late Google script cannot restore Chromium's
    // sheet (OSR cannot show it).
    try {
      Object.defineProperty(obj, name, {
        configurable: true, enumerable: true,
        get: function(){ return fn; },
        set: function(){}
      });
    } catch (e) {
      try { obj[name] = fn; } catch (e2) {}
    }
  }
  if (proto) {
    installMethod(proto, 'get', hookedGet);
    installMethod(proto, 'create', hookedCreate);
  }
  installMethod(navigator.credentials, 'get', hookedGet);
  installMethod(navigator.credentials, 'create', hookedCreate);
  try {
    if (window.PublicKeyCredential) {
      // Steer Google off native/hybrid sheets (OSR cannot show them).
      PublicKeyCredential.isUserVerifyingPlatformAuthenticatorAvailable = function(){
        return Promise.resolve(true);
      };
      PublicKeyCredential.isConditionalMediationAvailable = function(){
        return Promise.resolve(false);
      };
      if (typeof PublicKeyCredential.getClientCapabilities === 'function') {
        PublicKeyCredential.getClientCapabilities = function(){
          return Promise.resolve({
            conditionalCreate: false,
            conditionalGet: false,
            hybridTransport: false,
            passkeyPlatformAuthenticator: true,
            userVerifyingPlatformAuthenticator: true,
            relatedOrigins: false,
            signalAllAcceptedCredentials: false,
            signalCurrentUserDetails: false,
            signalUnknownCredential: false
          });
        };
      }
    }
  } catch (e) {}
})();"#
}

/// JS that settles the page's request `nonce` (payload is a JSON string on
/// success, an error message otherwise). Evaluate it in the requesting
/// frame only.
pub fn resolve_webauthn_script(nonce: &str, ok: bool, payload_json: &str) -> String {
    let nonce = serde_json::to_string(nonce).unwrap_or_else(|_| "\"\"".into());
    let payload = serde_json::to_string(payload_json).unwrap_or_else(|_| "\"\"".into());
    let ok_js = if ok { "true" } else { "false" };
    format!(
        "(function(){{ try {{ if (window.__solaWebAuthnResolve) {{ window.__solaWebAuthnResolve({nonce}, {ok_js}, {payload}); }} }} catch(e) {{ console.error('__sola_webauthn_resolve_err__' + e); }} }})();"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_public_key_does_not_fall_through_to_chromium() {
        let s = inject_webauthn_intercept_script();
        let create = s
            .split("function hookedCreate")
            .nth(1)
            .and_then(|c| c.split("function installMethod").next())
            .expect("create hook");
        assert!(
            create.contains("ask('create'"),
            "publicKey create() must post to chrome and hold the promise"
        );
        // origCreate.call is only the non-publicKey fallback.
        assert_eq!(create.matches("origCreate.call(").count(), 1);
        let ask = s.split("function ask(").nth(1).expect("ask()");
        assert!(ask.contains("publicKey: serializePk(publicKey)"));
        assert!(ask.contains("return new Promise"));
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
    fn requests_go_through_the_native_post_only() {
        let s = inject_webauthn_intercept_script();
        let post = s.split("function post(req)").nth(1).expect("post()");
        let post = post.split("function ask(").next().unwrap();
        assert!(post.contains("nativePost(JSON.stringify(req))"));
        assert!(!s.contains("console.log(s)"), "no console transport");
        assert!(!s.contains("sola.invalid"), "no iframe beacon");
        assert!(
            !s.contains("origin: location.origin"),
            "the helper takes the origin from the frame"
        );
        assert!(s.contains("crypto.getRandomValues"), "random nonce per request");
        assert!(s.contains("action: 'cancel'"), "tell chrome when the page gives up");
    }

    #[test]
    fn hooks_prototype_and_advertises_platform_authenticator() {
        let s = inject_webauthn_intercept_script();
        assert!(
            s.contains("CredentialsContainer.prototype"),
            "instance assignment is not enough — Google snapshots get()"
        );
        assert!(
            s.contains("isUserVerifyingPlatformAuthenticatorAvailable"),
            "OSR has no platform authenticator; advertise one so Google calls get()"
        );
        assert!(s.contains("isConditionalMediationAvailable"));
        assert!(
            s.contains("hybridTransport: false"),
            "hybrid/QR sheet cannot show in OSR"
        );
        assert!(
            s.contains("get: function(){ return fn; }"),
            "getter lock so the page cannot restore native get"
        );
    }

    #[test]
    fn resolve_script_escapes_the_nonce_and_payload() {
        let js = resolve_webauthn_script("ab\"c", false, "No'pe");
        assert!(js.contains(r#"__solaWebAuthnResolve("ab\"c", false, "No'pe")"#), "{js}");
    }
}
