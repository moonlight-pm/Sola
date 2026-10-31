# The call plane

`sola-call` is Sola's request/reply plane. One process, `sola-call`, holds a
live registry of methods. A process that offers methods (an **owner**)
connects and advertises them; a **caller** sends a request to one owner and
gets exactly one reply or an error; an **observer** watches the registry and
every call. `solactl` and its MCP server are callers, as is the shell (for
screenshots, and for the launcher's Projects row: `window.navigate`).
Monitor is an observer.

The command reference for `solactl` is [solactl](solactl.md). This chapter
explains the plane it talks to.

## Why it is separate from the bus

The bus ([The bus](bus.md)) fans out facts: anyone may emit a topic, every
subscriber gets it, and there is no request id, no reply, and no way to
tell that nobody was listening. That is right for announcements and sticky
state (windows, focus, theme, menus, `LaunchApp`).

Some operations need an answer for one caller: take a screenshot and say
where it went, click here, open this tab, tell me if it worked. The call
plane gives each request an id, a deadline, a named owner, and one reply.
When the owner is not connected, the caller gets an error at once. The
host never starts an application to answer a call. Screenshots, synthetic
input, and app control (Projects, Browser, Design, Bots) are calls; none
of them are bus topics.

The bus, the call plane, and Wayland are the three planes of a session
([Architecture](architecture.md)).

## The host

On Oath, PID 1 runs `sola-call` as a service and starts it again when it
exits with an error ([Architecture](architecture.md)).

- **Socket.** `$SOLA_CALL_PATH` if set, else `$XDG_RUNTIME_DIR/sola-call`.
  When `XDG_RUNTIME_DIR` is unset, the runtime directory is
  `/run/user/<uid>` if that exists, else `/tmp`. At start the host
  removes any old socket file, creates the parent directory, binds, and
  sets mode `0600`. Any process of the same user can connect; there is no
  other access control.
- **State.** The registry (owner, app id, methods), the calls in flight,
  and the observers live in memory, one thread per connection. Nothing is
  saved. After a restart of `sola-call` the registry is empty; owners
  reconnect and advertise again within about 400 ms.
- **No confirm.** No method asks a person before it runs. Which methods
  should is an open question
  ([D3](../open-questions.md#d3--which-call-plane-methods-need-a-human-confirm-p1)).

## Roles and the handshake

Every connection starts with a `hello` frame that names its role and its
`app_id`. A connection whose first frame is anything else is closed.

| Role | Hello carries | Then sends | Receives |
|---|---|---|---|
| `provider` | `app_id`, `owner` | `advertise` at once; later `reply` frames | `invoke` frames |
| `caller` | `app_id` | `invoke` and `list`, any number | `reply` and `catalog` |
| `observer` | `app_id` | `list` (optional) | `catalog` and `trace` |

The **owner** is the noun callers use (`compositor`); the **app id** is the
process (`sola-river`). A provider whose hello has no `owner`, or whose
second frame is not `advertise`, is closed.

One provider holds an owner name. A later provider that says hello with the
same owner replaces the earlier one. Limit: when the replaced connection
then closes, the host removes the owner from the registry even though the
newer provider is still connected; the owner stays missing until that
provider's connection is made again.

## Methods and arguments

A provider advertises a list of method specs.

| `MethodSpec` field | Meaning |
|---|---|
| `name` | Method name within the owner: `windows`, `tab.open`, `input.click` |
| `summary` | One line, shown by `solactl <owner>` and in MCP tool descriptions |
| `args` | Argument specs (below) |
| `timeout_ms` | Suggested deadline for callers; omitted means 8 s |

Names are lower case; a dotted name groups related methods (`pane.list`,
`pane.send`). Monitor and this manual write a call as `owner.method`
(`compositor.screenshot`).

| `ArgSpec` field | Meaning |
|---|---|
| `name` | The key in the request's `params` object |
| `long` | The `--flag` that `solactl` accepts |
| `short` | An optional one-letter `-f` |
| `type` | `string`, `int`, `float`, `bool`, `path`, or `json` |
| `required` | Marks the argument required in help and MCP schemas, and lets `solactl` fill it from a bare word |
| `help` | Shown by `solactl <owner> <method> --help` and in the MCP schema |

The host checks only that the method name is in the owner's advertised
list. It does not check arguments. Each owner reads the keys it knows,
ignores the rest, and answers a missing required argument with an error
reply; how it treats a value of the wrong type is up to that owner.

`path` is a string with one difference: `solactl` and the MCP server make
it absolute against the caller's working directory before the call (`~`
expands too), because the owner runs in its own process with its own
working directory.

## Keeping the catalog live

The catalog is the set of connected providers. An owner's entry lasts as
long as its connection: when the provider exits, crashes, or its
connection breaks, the host removes it and tells the observers.

The client library reconnects a provider every 400 ms while it is not
connected, then sends `hello` and `advertise` again. An app that starts
before `sola-call`, or outlives a restart of it, appears as soon as the
socket answers.

A provider may send `advertise` again on the same connection to replace its
list. The client library does not, so an owner's methods are fixed for as
long as its process runs. `list` returns the catalog sorted by owner.

## Wire format

Each frame is a 4-byte little-endian unsigned length followed by that many
bytes of UTF-8 JSON. A reader refuses a frame over 16 MiB and closes that
connection, so large results travel as file paths, not bytes (screenshots
reply with a `path`).

Every message is a JSON object with a `type`:

| `type` | Fields | Sent by |
|---|---|---|
| `hello` | `role`, `app_id`, `owner` (providers) | everyone, first |
| `advertise` | `methods` | provider |
| `invoke` | `id`, `method`, `params`, `owner`, `timeout_ms` | caller; host to provider |
| `reply` | `id`, `ok`, `error` or `data` | provider; host to caller |
| `list` | none | caller or observer |
| `catalog` | `owners`: `owner`, `app_id`, `methods` | host |
| `trace` | the trace event fields | host to observers |

A call, as the frames' JSON:

```text
caller → host     {"type":"hello","role":"caller","app_id":"solactl"}
caller → host     {"type":"invoke","id":"0199…","owner":"compositor",
                   "method":"windows","params":{},"timeout_ms":5000}
host → provider   {"type":"invoke","id":"0199…","method":"windows",
                   "params":{},"timeout_ms":5000}
provider → host   {"type":"reply","id":"0199…","ok":true,
                   "data":{"sola-shell":[{"title":"…","window_id":…,"pid":…}]}}
host → caller     {"type":"reply","id":"0199…","ok":true,"data":{…}}
```

- **Ids.** The caller chooses the request id; the client library uses a
  UUIDv7 string. The host tracks calls in flight by id across all callers,
  so ids must be unique. The host forwards the invoke under the same id,
  without `owner`, with the caller's `timeout_ms`.
- **Params.** Any JSON value; owners expect an object.
- **Replies.** `ok: true` with `data` (any JSON, `null` included), or
  `ok: false` with an `error` string. Only the first reply for an id
  counts; later replies for it are discarded.

The host answers some invokes itself:

| Error | When |
|---|---|
| `invoke missing owner` | The invoke has no `owner` |
| `<owner> is not running` | No provider holds that owner |
| `<owner> has no method <method>` | The owner did not advertise that name |
| `timeout` | The deadline passed with no reply |

## Deadlines and failures

Each invoke carries `timeout_ms`; without one the host uses 8000. The host
checks deadlines every 200 ms, so a `timeout` reply arrives up to about
200 ms after the deadline. The owner is not told that the caller gave up;
a reply that comes later is discarded.

| Situation | What the caller sees |
|---|---|
| `sola-call` is not running | Connecting fails: `call host is not running (<socket path>)` |
| Owner not connected | At once: `<owner> is not running` |
| Method not advertised | At once: `<owner> has no method <method>` |
| Owner slow | `timeout` at the deadline |
| Owner exits or crashes during the call | `timeout` at the deadline. The owner leaves the catalog at once, but its pending calls wait out their deadlines |
| Kit app before its UI loop is running | The call is dropped: `timeout` at the deadline |
| Kit app whose UI loop stopped taking calls | At once: `the app is running but its UI loop is not draining calls. Close that window (window X or Flower) and launch it again. Do not kill the process from a bot.` |
| `sola-call` restarts during the call | `call host closed the connection` |

How `solactl` turns these into messages and exit statuses is in
[solactl § Output and exit status](solactl.md#output-and-exit-status).

## Observers

An observer has the same privilege as a caller and sees all traffic: every
invoke's `params` and every reply's `data`.

- **Catalog.** The host sends a `catalog` on connect, after every
  `advertise`, and after every owner leaves. An observer may also send
  `list` for one.
- **Traces.** The host sends a `trace` for every event:

| `kind` | When | Fields set |
|---|---|---|
| `invoke` | A caller's invoke reaches the host | `id`, `owner`, `caller`, `method`, `params` |
| `reply` | A reply is passed back, or the host refuses the invoke because the owner or method is missing (`duration_ms` 0) | `id`, `owner`, `caller`, `method`, `ok`, `error`, `data`, `duration_ms` |
| `timeout` | A deadline passes | `id`, `owner`, `caller`, `method`, `ok: false`, `error: "timeout"`, `duration_ms` |
| `advertise` | A provider advertises | `owner`, `data` (its methods) |
| `unregister` | An owner leaves | `owner` |

`caller` is the `app_id` from the caller's hello. Limit: the library's
one-shot `invoke` always says hello as `solactl`, so calls from the shell
and the MCP server also trace as `solactl`.

[Monitor](sola-monitor.md) is the observer Sola has: its **Call** view
lists invokes with owner, method, caller, status, and duration, and its
right rail is the live catalog.

## Owners today

| Owner | Process | Present | Methods |
|---|---|---|---|
| `compositor` | `sola-river` | Whole session | `screenshot`, `sample`, `cursor`, `windows`, `input.click`, `input.press`, `input.release`, `input.move`, `input.scroll`, `input.key` |
| `session` | `sola-session` | Whole session | `launch`, `close` |
| `bots` | `sola-botsd` | While `sola-botsd` runs | `list`, `poll`, `new`, `setup`, `send`, `transcript`, `rm`, `cancel` |
| `projects` | `sola-projects` | While **Projects** runs; it quits when its last project window closes | `ps`, `project.list`, `window.navigate`, `project.open`, `project.close`, `tab.list`, `tab.spawn`, `tab.rm`, `tab.select`, `tab.set`, `tab.exec`, `pane.list`, `pane.send`, `pane.read`, `pane.wait`, `whoami` |
| `browser` | `sola-browser` | While the **Browser** runs | `tabs`, `tab.open`, `tab.close`, `tab.focus`, `goto`, `back`, `forward`, `reload`, `stop`, `find.page`, `snapshot`, `find`, `click`, `hover`, `key`, `scroll`, `type`, `fill`, `get`, `links`, `select`, `wait`, `screenshot` |
| `design` | `sola-design` | While **Design** runs | `project.list`, `project.add`, `project.remove`, `open`, `canvas.new`, `canvas.close`, `status`, `share`, `share.rotate`, `selection`, `view`, `wait`, `reload`, `screenshot`, `pdf`, `inspect` |

The compositor and session owners belong to services PID 1 keeps running.
`sola-botsd` ships with Sola, but Oath runs no service for it, so `bots`
is present only while you run it. The other three exist only while their
application runs; the plane does not start it for you. Every method and
flag is in [solactl](solactl.md).

## solactl on the call plane

`solactl` has two kinds of call commands.

- **Compiled commands**, `compositor` and `session`, are parsed by
  `solactl` itself: fixed flags, defaults, and `--help` that works with
  nothing running. `solactl` builds the params and invokes the method.
- **Live-owner commands**, `projects`, `browser`, `bots`, `design`, and
  any other name, are read from the catalog. `solactl <owner>` lists the
  owner's methods and `solactl <owner> <method> --help` its flags, both
  from the advertised specs. The name may be the owner or its app id.
  These need `sola-call` and the owner running, even for help.

For a live-owner command, `solactl` turns flags into params by the
method's `ArgSpec`s: `--long VALUE` or `-s VALUE`, bool flags with no
value, a bare word for the next required argument not yet given, or a
single JSON object instead of flags. Values are converted by `type`, and
`path` values are made absolute against your working directory. It adds a
little context of its own: in a Projects pane, `whoami` and
`tab.spawn` get your pane from `$SOLA_PANE_ID` (and `whoami` your
checkout from `$SOLA_WS_PATH`); `design canvas.new` without `--path` uses
your working directory.

The deadline is the method's `--timeout` argument plus 2 s (at least 8 s)
when you pass one, else its advertised `timeout_ms` rounded up to whole
seconds, else 8 s. The reply's `data` is printed as indented JSON. The
full rules are in [solactl § Live-owner commands](solactl.md#live-owner-commands).

## MCP (`solactl mcp`)

`solactl mcp` is an MCP server on stdio for agents. It is an adapter over
the same methods, not a second protocol: what a tool may do is what
`solactl` may do.

- **Transport.** JSON-RPC 2.0, one message per line. It answers
  `initialize`, `ping`, `tools/list`, and `tools/call`; any other method
  gets error `-32601`, and a line that is not JSON gets `-32700`. It
  offers protocol versions `2025-06-18`, `2025-03-26`, and `2024-11-05`
  (the client's if it asks for one of these, else the newest). Its
  `serverInfo.name` is `sola`, and its `instructions` explain the tool
  names and how to work with Design.
- **Tools.** One tool per live method, named `<owner>_<method>`: every
  character other than a letter, digit, or `-` becomes `_`, and the name
  is cut at 64 characters (`browser_tab_open`, `design_screenshot`). The
  description is the summary, the app id, and the matching `solactl`
  command. `--owner NAME` (repeatable; owner or app id) limits the tools to
  those owners.
- **Schemas.** Each `ArgSpec` is a property named by `name`: `string` and
  `path` are JSON strings (a path's description ends in `(path)`), `int`
  is `integer`, `float` is `number`, `bool` is `boolean`, and `json` is
  `object`. Required arguments are listed in `required`.
- **Calls.** Arguments must be a JSON object or absent. They are passed as
  sent, with no type conversion, except that `path` arguments are made
  absolute against the server's working directory (the agent's). The
  Projects and Design context, the bot rules for the Browser, and the
  deadline are the same as for `solactl`. A Design page call without
  `canvas` goes to the canvas this server last opened or created. Each call
  runs on its own thread, so `ping` and `tools/list` are answered while a
  long call waits.
- **Results.** A reply becomes a text item holding the indented JSON (`ok`
  for `null`). When the reply has a `path` ending in `.png`, `.jpg`, or
  `.jpeg` and the file is at most 3,700,000 bytes, the image is attached
  as a second, base64 item; a larger file gets a text note with its size
  instead. An error is a text item with `isError: true`; a call whose
  deadline passes says `timeout waiting for <owner>.<method>`, with a note
  that an application still on screen has a wedged call channel and should
  be closed from its window and launched again.
- **Changes.** The server rereads the catalog every 2 s and sends
  `notifications/tools/list_changed` when the set of tool names changes,
  once the client has sent `notifications/initialized`. When `sola-call`
  is down, `tools/list` is empty; calling a tool whose owner is gone
  returns this error:
  `no live tool "<name>" — is that app running? (tools follow the apps that are up)`.

Limit: the compositor's `SOLA_BOT=1` refusals belong to the compiled
`solactl compositor` command, so a bot's MCP server still offers the
compositor tools. Register the server with an agent as a stdio command,
for example `claude mcp add sola -- /bin/solactl mcp`.

## Offering methods from an application

A kit application advertises its methods when it sets up the bus, then
handles calls in its iced loop.

- `BusSetup::calls(owner, methods)` before `install()` starts the provider
  once the bus is connected. `CallSetup::new(owner, app_id)`, then
  `.methods(…)` and `.install()`, does the same without the bus.
- `sola_kit::call_subscription()` is a `Subscription<Incoming>`; fold it
  into the application's subscription. Use it or drain the receiver
  yourself, not both; a process has one call receiver.
- An `Incoming` has `method`, `params`, and `reply`. Answer with
  `reply.ok(json)` or `reply.err(message)`, exactly once. A reply that is
  dropped unanswered leaves the caller waiting for its deadline.
- A process that is not a kit app calls
  `sola_call::start_provider(owner, app_id, methods)` and drains the
  returned channel in its own loop, as `sola-river`, `sola-session`, and
  `sola-botsd` do.
- A caller uses `sola_call::invoke(owner, method, params, timeout)` and
  `sola_call::catalog()`; an observer uses `sola_call::start_observer`, or
  the kit's `install_observer` and `observe_subscription`.

```text
use sola_kit::sola_call::{ArgSpec, ArgType, Incoming, MethodSpec};

fn methods() -> Vec<MethodSpec> {
    vec![MethodSpec {
        name: "note.add".into(),
        summary: "Add a note".into(),
        args: vec![ArgSpec {
            name: "text".into(),
            long: Some("text".into()),
            short: Some('t'),
            ty: ArgType::String,
            required: true,
            help: "Note text".into(),
        }],
        timeout_ms: None,
    }]
}

// main, before iced starts
sola_kit::BusSetup::new(APP_ID)
    .calls("notes", methods())
    .install();

// subscription
sola_kit::call_subscription().map(Msg::Call)

// update
Msg::Call(inc) => match inc.method.as_str() {
    "note.add" => match inc.params.get("text").and_then(|v| v.as_str()) {
        Some(text) => {
            self.add(text);
            inc.reply.ok(serde_json::json!({ "ok": true }));
        }
        None => inc.reply.err("text required"),
    },
    other => inc.reply.err(format!("unknown method {other}")),
},
```

With that application open, `solactl notes note.add --text hi` (or
`solactl notes note.add hi`) calls it, and an agent sees the tool
`notes_note_add`.

Some choices keep a method easy to call:

- Keep `long` equal to `name`. `solactl` sends a `--long` value under the
  flag's own text, so a different `long` reaches the owner under a key it
  does not expect.
- Do not use `h` as a short flag; `solactl` reads `-h` as help.
- Give a method that can run longer than 8 s a `timeout_ms`, or a
  `timeout` argument in seconds, which `solactl` and MCP turn into the
  deadline.
- Reply with a file path for large results.
