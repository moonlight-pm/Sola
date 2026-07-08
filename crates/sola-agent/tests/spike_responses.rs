//! PHASE-1 SPIKE (ignored by default; needs network + a real key).
//!
//! Confirms the Sakana Fugu *Responses* streaming contract BEFORE the engine is
//! built on it. Run it once, by hand, and read the printed SSE:
//!
//!   SAKANA_API_KEY=sk-... cargo test -p sola-agent --test spike_responses \
//!       -- --ignored --nocapture
//!
//! WHAT TO LOOK FOR in the printed `event:` lines (these names are the single
//! fact the whole engine depends on — confirm each appears, and that the
//! function-call round-trip matches):
//!   * response.output_text.delta             -> streamed assistant text (data.delta)
//!   * response.output_item.added             -> a function_call item begins
//!         (data.item.type == "function_call"; carries item.call_id + item.name)
//!   * response.function_call_arguments.delta -> incremental args, keyed by data.item_id
//!   * response.function_call_arguments.done  -> args finished (data.item_id + data.arguments)
//!   * response.output_item.done              -> the FINISHED function_call item
//!         AUTHORITATIVE: data.item.{call_id,name,arguments} (full arguments string)
//!   * response.completed                     -> data.response.usage.{input_tokens,output_tokens}
//! If any name differs, update `provider::parse_sse_event` to match before Task C.

use std::io::{BufRead, BufReader};

#[test]
#[ignore = "live network + real SAKANA_API_KEY; run by hand to confirm the SSE contract"]
fn spike_responses_function_call_roundtrip() {
    // Deterministic crypto backend from a bare TTY (matches the app).
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    let key = std::env::var("SAKANA_API_KEY").expect("set SAKANA_API_KEY to run the spike");

    let body = serde_json::json!({
        "model": "fugu",
        "input": [{
            "role": "user",
            "content": [{ "type": "input_text",
                          "text": "Call the get_weather tool for Tokyo, then stop." }]
        }],
        "tools": [{
            "type": "function",
            "name": "get_weather",
            "description": "Get the current weather for a city.",
            "parameters": {
                "type": "object",
                "properties": { "city": { "type": "string" } },
                "required": ["city"],
                "additionalProperties": false
            },
            "strict": true
        }],
        "stream": true,
        "store": false,
        "reasoning": { "effort": "high" }
    });

    let mut resp = ureq::post("https://api.sakana.ai/v1/responses")
        .header("Authorization", &format!("Bearer {key}"))
        .header("Accept", "text/event-stream")
        .send_json(&body)
        .expect("POST /responses failed");

    eprintln!("=== HTTP {} ===", resp.status());
    let reader = BufReader::new(resp.body_mut().as_reader());
    let mut saw_completed = false;
    for line in reader.lines() {
        let line = line.expect("read SSE line");
        if line.starts_with("event:") || line.starts_with("data:") {
            eprintln!("{line}");
        }
        if line.contains("response.completed") {
            saw_completed = true;
        }
    }
    assert!(saw_completed, "stream ended without a response.completed event");
}
