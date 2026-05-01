/// Strip TypeScript type annotations, returning JavaScript.
/// Uses swc_ts_fast_strip in StripOnly mode (whitespace replacement).
pub fn strip_ts(source: &str) -> String {
    use swc_common::SourceMap;
    use swc_common::errors::Handler;
    use swc_common::sync::Lrc;
    use swc_ts_fast_strip::{Mode, Options, operate};

    let cm: Lrc<SourceMap> = Default::default();
    let handler = Handler::with_emitter_writer(Box::new(std::io::sink()), Some(cm.clone()));

    match operate(
        &cm,
        &handler,
        source.to_string(),
        Options {
            mode: Mode::StripOnly,
            ..Default::default()
        },
    ) {
        Ok(output) => output.code,
        Err(e) => {
            tracing::error!("TS strip failed: {e:?}");
            source.to_string()
        }
    }
}
