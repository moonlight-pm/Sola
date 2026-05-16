// JSON syntax highlighter. Produces an array of <span> nodes with
// `.token-<kind>` classes that lib/style.css colors. Port of the
// legacy `tokenizeJson` helper, with the array return shape adapted
// to Remix v3 (returns an array of RemixNode, not arrow-js html`…`).

import { type RemixNode } from "@remix-run/ui";

export function highlightedPreview(payload: unknown): RemixNode {
  if (payload == null) return "";
  return tokenizeJson(JSON.stringify(payload), 200);
}

export function highlightedJson(payload: unknown): RemixNode {
  if (payload == null) return "";
  return tokenizeJson(JSON.stringify(payload, null, 2));
}

export function tokenizeJson(json: string, maxChars?: number): RemixNode {
  const tokens: RemixNode[] = [];
  let i = 0;
  let emittedChars = 0;
  const out = (text: string, kind: string | null) => {
    if (maxChars !== undefined) {
      const remaining = maxChars - emittedChars;
      if (remaining <= 0) return false;
      if (text.length > remaining) text = text.slice(0, remaining) + "…";
    }
    emittedChars += text.length;
    if (kind) tokens.push(<span class={`token-${kind}`}>{text}</span>);
    else tokens.push(text);
    return maxChars === undefined || emittedChars < maxChars;
  };

  while (i < json.length) {
    const ch = json[i];
    if (ch === '"') {
      // String literal — peek ahead to decide if it's a key or a value.
      let j = i + 1;
      while (j < json.length) {
        if (json[j] === "\\") { j += 2; continue; }
        if (json[j] === '"') break;
        j++;
      }
      const lit = json.slice(i, j + 1);
      // A key is a string followed (after optional whitespace) by ":".
      let k = j + 1;
      while (k < json.length && /\s/.test(json[k])) k++;
      const isKey = json[k] === ":";
      if (!out(lit, isKey ? "key" : "string")) return tokens;
      i = j + 1;
    } else if (/[\d\-]/.test(ch)) {
      let j = i;
      while (j < json.length && /[\d.eE+\-]/.test(json[j])) j++;
      if (!out(json.slice(i, j), "number")) return tokens;
      i = j;
    } else if (json.startsWith("true", i) || json.startsWith("false", i)) {
      const word = json.startsWith("true", i) ? "true" : "false";
      if (!out(word, "boolean")) return tokens;
      i += word.length;
    } else if (json.startsWith("null", i)) {
      if (!out("null", "null")) return tokens;
      i += 4;
    } else {
      if (!out(ch, null)) return tokens;
      i++;
    }
  }
  return tokens;
}
