// TokenValueEditor — picks the right input primitive for a Token
// based on its `kind` and forwards value / onChange:
//
//   Color       → <ColorInput>
//   FontFamily  → <FontInput>
//   TextSize    → <NumberInput unit="px" min={0}>
//   Space       → <NumberInput unit="px" min={0}>
//   Radius      → <NumberInput unit="px" min={0}>
//
// Shared between the Tokens page (which edits an atom directly)
// and the BindingsEditor (which edits the atom currently bound to
// a component slot). Lifting this into its own component keeps
// the kind→input mapping in one place — adding a new TokenKind
// only requires touching this file.

import { type Handle } from "@remix-run/ui";
import { ColorInput } from "@sola/color-input";
import { FontInput } from "@sola/font-input";
import { type Token } from "@sola/kit";
import { NumberInput } from "@sola/number-input";
import { TextInput } from "@sola/text-input";

export interface TokenValueEditorProps {
  token: Token;
  onChange?: (value: string) => void;
}

export function TokenValueEditor(handle: Handle<TokenValueEditorProps>) {
  return () => {
    const { token, onChange } = handle.props;
    switch (token.kind) {
      case "Color":
        return <ColorInput value={token.value} onChange={onChange} />;
      case "FontFamily":
        return <FontInput value={token.value} onChange={onChange} />;
      case "TextSize":
      case "Space":
      case "Radius":
        return (
          <NumberInput
            value={token.value}
            unit="px"
            min={0}
            onChange={onChange}
          />
        );
      default:
        // Unknown kind — fall back to a plain text input rather
        // than refusing to render. This is what callers see if a
        // future TokenKind variant lands before this file knows
        // about it.
        return <TextInput value={token.value} onChange={onChange} />;
    }
  };
}
