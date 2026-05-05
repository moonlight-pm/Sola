//! Per-request transform of TS/TSX/JSX source to plain JS.
//!
//! Pipeline:
//!   .ts  → strip types
//!   .tsx → JSX→jsx()/jsxs() (auto-imported) then strip types
//!   .jsx → JSX→jsx()/jsxs() (auto-imported)
//!
//! JSX transform is configured for Preact's automatic runtime
//! (`import_source: "preact"`), so swc auto-injects
//! `import { jsx, jsxs, Fragment } from "preact/jsx-runtime"` for any
//! file containing JSX. App code never needs to import `h` or `Fragment`.

use swc_core::common::comments::SingleThreadedComments;
use swc_core::common::sync::Lrc;
use swc_core::common::{FileName, GLOBALS, Globals, Mark, SourceMap};
use swc_core::ecma::ast::{EsVersion, Program};
use swc_core::ecma::codegen::Emitter;
use swc_core::ecma::codegen::text_writer::JsWriter;
use swc_core::ecma::parser::lexer::Lexer;
use swc_core::ecma::parser::{Parser, StringInput, Syntax, TsSyntax};
use swc_core::ecma::transforms::base::resolver;
use swc_core::ecma::transforms::react::{Options as JsxOptions, Runtime, jsx};
use swc_core::ecma::transforms::typescript::strip;

/// Strip TypeScript and/or transform JSX, returning JavaScript.
///
/// `has_jsx` enables the TSX/JSX parser path and the JSX→h() transform.
/// `has_types` enables the TS type-strip pass.
pub fn transform(source: &str, has_jsx: bool, has_types: bool) -> String {
    let cm: Lrc<SourceMap> = Default::default();
    let fm = cm.new_source_file(
        Lrc::new(FileName::Custom("inline.ts".into())),
        source.to_string(),
    );

    let lexer = Lexer::new(
        Syntax::Typescript(TsSyntax {
            tsx: has_jsx,
            ..Default::default()
        }),
        EsVersion::EsNext,
        StringInput::from(&*fm),
        None,
    );
    let mut parser = Parser::new_from(lexer);
    let module = match parser.parse_module() {
        Ok(m) => m,
        Err(e) => {
            tracing::error!("swc parse failed: {e:?}");
            return source.to_string();
        }
    };

    let globals = Globals::default();
    GLOBALS.set(&globals, || {
        let unresolved_mark = Mark::new();
        let top_level_mark = Mark::new();
        let comments = SingleThreadedComments::default();

        let mut program = Program::Module(module);

        // Resolver assigns scope marks to every identifier. Without it,
        // identifiers JSX inserts (e.g. `h` from `<Foo />` → `h(Foo, ...)`)
        // are unmarked, and the TS strip pass's import-elision sees no
        // "value use" of `h` and removes the import → ReferenceError at runtime.
        program = program.apply(resolver(unresolved_mark, top_level_mark, true));

        if has_jsx {
            program = program.apply(jsx(
                cm.clone(),
                Some(&comments),
                JsxOptions {
                    runtime: Some(Runtime::Automatic),
                    import_source: Some("preact".into()),
                    ..Default::default()
                },
                top_level_mark,
                unresolved_mark,
            ));
        }

        if has_types {
            program = program.apply(strip(unresolved_mark, top_level_mark));
        }

        let module = match program {
            Program::Module(m) => m,
            Program::Script(_) => return source.to_string(),
        };

        let mut buf = vec![];
        {
            let mut emitter = Emitter {
                cfg: Default::default(),
                cm: cm.clone(),
                comments: Some(&comments),
                wr: JsWriter::new(cm.clone(), "\n", &mut buf, None),
            };
            if emitter.emit_module(&module).is_err() {
                return source.to_string();
            }
        }
        String::from_utf8(buf).unwrap_or_else(|_| source.to_string())
    })
}

/// Back-compat entry point: TS-only, no JSX.
pub fn strip_ts(source: &str) -> String {
    transform(source, false, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsx_auto_imports_jsx_runtime() {
        // App code does NOT import h. Automatic runtime should add
        // `import { jsx } from "preact/jsx-runtime"` (or jsxs) for us.
        let src = r#"import { render } from "preact";
import { Main } from "./components/Main";

render(<Main />, document.body);
"#;
        let out = transform(src, true, true);
        eprintln!("=== TRANSFORM OUTPUT ===\n{}\n=== END ===", out);
        assert!(
            out.contains("preact/jsx-runtime"),
            "expected automatic-runtime import from preact/jsx-runtime; got:\n{out}"
        );
        // JSX must lower to a jsx*() call from the runtime.
        assert!(
            out.contains("jsx(") || out.contains("jsxs("),
            "expected jsx()/jsxs() call from automatic runtime; got:\n{out}"
        );
        // The user's existing `render` import must survive resolver+strip.
        assert!(
            out.contains("import { render }") || out.contains("render,") || out.contains(", render"),
            "expected user's `render` import to survive; got:\n{out}"
        );
    }
}
