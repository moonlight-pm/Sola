//! Per-request transform of TS/TSX/JSX source to plain JS.
//!
//! Pipeline:
//!   .ts  → strip types
//!   .tsx → JSX→h() then strip types
//!   .jsx → JSX→h()
//!
//! JSX transform is configured for Preact (classic runtime, pragma "h",
//! fragment "Fragment"). Apps just need `import { h, Fragment } from 'preact'`
//! at the top of any .tsx file.

use swc_core::common::comments::SingleThreadedComments;
use swc_core::common::sync::Lrc;
use swc_core::common::{FileName, GLOBALS, Globals, Mark, SourceMap};
use swc_core::ecma::ast::{EsVersion, Program};
use swc_core::ecma::codegen::Emitter;
use swc_core::ecma::codegen::text_writer::JsWriter;
use swc_core::ecma::parser::lexer::Lexer;
use swc_core::ecma::parser::{Parser, StringInput, Syntax, TsSyntax};
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

        if has_jsx {
            program = program.apply(jsx(
                cm.clone(),
                Some(&comments),
                JsxOptions {
                    runtime: Some(Runtime::Classic),
                    pragma: Some("h".into()),
                    pragma_frag: Some("Fragment".into()),
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
