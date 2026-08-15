# solactl

Operator CLI. Talks to **sola-call** for compositor/session verbs, and to
the **bus** only for `emit`.

## Call plane (needs `sola-call` + the owner process)

```text
solactl compositor screenshot [-o PATH] [--app APP] [--window TITLE]
solactl compositor windows
solactl compositor input click|move|scroll|key …
solactl session launch <app_id> [--command CMD]
solactl session close  <app_id>
```

If `sola-call` or the owner is down, the command fails. It does **not**
launch a window.

Other running apps that have advertised methods: `solactl <app-id>` lists
them; `solactl <app-id> <method> …` invokes.

## Not calls

```text
solactl emit <Topic> '<json>'   # bus poke
solactl logs [app] [-f]         # /opt/sola/log
solactl open <url|path>         # sola-browser (http/https) or sola-paint (image path)
solactl media <action>          # MPRIS / wpctl (shell key handler)
```

`eval` is gone (WebView stack retired). Screenshot and synthetic input
are calls, not bus topics.
