# ADR-0094: An email destination writes one MIME message and sends it by SMTP or a provider's API

**Status:** Accepted
**Date:** 2026-11-27

## Context

Email reaches people no chat does. herald's outbound ports are `Http`,
fenced to each destination's hosts (ADR-0007 decision 5), and `Sockets`,
which opens WebSocket connections; none carries SMTP. A store kind
declares what its host must offer beyond `Http` (`StoreKind::NEEDS`,
`Offer`), and each host lists what it offers; a destination kind declares
no such need.

What the mail standards and services say:

- SMTP submission: implicit TLS on 465 is preferred over STARTTLS on 587
  (<https://www.rfc-editor.org/rfc/rfc8314.html>); a man in the middle can
  strip `250 STARTTLS`, and after TLS the client sends EHLO again
  (<https://www.rfc-editor.org/rfc/rfc3207.html>); AUTH mechanisms that
  send a password in the clear are not used without TLS
  (<https://www.rfc-editor.org/rfc/rfc4954.html>). Exchange Online turns
  basic-auth SMTP AUTH off by default at the end of December 2026
  (<https://techcommunity.microsoft.com/blog/Exchange/updated-exchange-online-smtp-auth-basic-authentication-deprecation-timeline/4489835>);
  XOAUTH2 takes `https://outlook.office.com/SMTP.Send`, or app-only
  `SMTP.SendAsApp`
  (<https://learn.microsoft.com/en-us/exchange/client-developer/legacy-protocols/how-to-authenticate-an-imap-pop-smtp-application-by-using-oauth>),
  and Google `https://mail.google.com/`
  (<https://developers.google.com/workspace/gmail/imap/xoauth2-protocol>).
- APIs that take a whole message: Graph `sendMail` takes base64 MIME
  (<https://learn.microsoft.com/en-us/graph/api/user-sendmail?view=graph-rest-1.0&tabs=http>);
  Gmail `users.messages.send` takes `raw`
  (<https://developers.google.com/workspace/gmail/api/reference/rest/v1/users.messages>);
  SES v2 `SendEmail` takes raw content
  (<https://docs.aws.amazon.com/ses/latest/APIReference-V2/API_SendEmail.html>);
  Mailgun takes `messages.mime`
  (<https://documentation.mailgun.com/docs/mailgun/api-reference/send/mailgun/messages/post-v3--domain-name--messages-mime>).
- APIs that take a message's parts: Postmark's `POST /email` takes
  `From`, `To`, `Subject`, `HtmlBody`, `TextBody`, `Headers` as name and
  value pairs, `TrackOpens` and `TrackLinks`, and no whole message
  (<https://postmarkapp.com/developer/api/email-api>, "Send a single
  email"); it does not say whether a `Message-ID` header given there is
  kept. Resend's `POST /emails` takes `from`, `to`, `subject`, `html`,
  `text` and a `headers` object, and no whole message
  (<https://resend.com/docs/api-reference/emails/send-email>); Resend
  gives each message its Message-ID and returns it from
  `GET /emails/:id` and its webhooks, for the next send's `In-Reply-To`
  and `References`
  (<https://resend.com/changelog/message-id-for-sent-emails>, 2026-07-08).
- Threading: `In-Reply-To` is the parent's Message-ID, and `References`
  the parent's References and its Message-ID (RFC 5322 §3.6.4); headers
  hold CR and LF only as CRLF, lines at most 998 characters, folded before
  whitespace, non-ASCII in RFC 2047 encoded-words
  (<https://www.rfc-editor.org/rfc/rfc5322.html>,
  <https://www.rfc-editor.org/rfc/rfc2047.html>);
  multipart/alternative puts the plainest part first
  (<https://www.rfc-editor.org/rfc/rfc2046.html> §5.1.4).

Where a host can open a TCP connection for SMTP, read 2026-11-27:

| Host | How | In-place TLS (STARTTLS) | What is blocked or not known |
|------|-----|-------------------------|------------------------------|
| Actions, `herald server` | the OS's sockets | yes | GitHub documents no port rule for its runners (<https://docs.github.com/en/actions/reference/runners/github-hosted-runners>) |
| Lambda | the OS's sockets | yes | port 25 to public addresses (<https://docs.aws.amazon.com/AWSEC2/latest/UserGuide/ec2-resource-limits.html>, "Restriction on email sent using port 25"); 465 and 587 not named |
| Azure | the OS's sockets | yes | port 25 unsupported on PaaS; 587 "isn't restricted in Azure, regardless of the subscription type" (<https://learn.microsoft.com/en-us/troubleshoot/azure/virtual-network/troubleshoot-outbound-smtp-connectivity>) |
| Cloudflare Worker | `connect()` from `cloudflare:sockets` | yes, `secureTransport: "starttls"` and `startTls()` | port 25, Cloudflare's addresses, private addresses (<https://developers.cloudflare.com/workers/runtime-apis/tcp-sockets/>, "Blocked Ports and Destinations") |
| Node, Bun | `node:net`, `node:tls` | yes, `tls.connect({ socket })` (<https://nodejs.org/api/tls.html>) | none documented; Bun's `node:tls` is "partially implemented" (<https://bun.com/docs/runtime/nodejs-compat>) |
| Deno | `Deno.connect`, `Deno.connectTls`, `Deno.startTls` (<https://docs.deno.com/api/deno/~/Deno.startTls>) | yes | `--allow-net`; Deno Deploy's port rules are not documented |
| Vercel | a Node.js function | yes | port 25 only (<https://vercel.com/kb/guide/serverless-functions-and-smtp>); the Edge runtime has no `node:net` (<https://vercel.com/docs/functions/runtimes/edge>) |
| `wasi:http` component | none | no | the `wasi:http/proxy` world imports no `wasi:sockets` (<https://github.com/WebAssembly/wasi-http/blob/main/wit/proxy.wit>) |
| Spin | not documented | not known | its manifest's `allowed_outbound_hosts` names HTTP, Redis, MySQL and PostgreSQL only (<https://spinframework.dev/v3/manifest-reference>) |
| Fastly Compute | none documented | not known | no TCP socket API in its documentation |
| Netlify | a Node.js function, where herald's deploy already reaches Postgres through `pg` (`deploy/netlify/functions/herald.mjs`) | yes, as Node | Netlify documents no port rule |

## Decision

1. **`kind = "email"`, in `herald-dest-email`, writes one RFC 5322
   message** and `transport` decides how it leaves.
2. **The message.**
   - `Subject`: the rendered title, every control character removed, one
     line, RFC 2047-encoded where it is not ASCII, folded at 78.
   - `Message-ID: <hex(announcement marker)@<from domain>>`; each later
     part carries `In-Reply-To` and `References`, so the destination
     threads by replies.
   - `Auto-Submitted: auto-generated`, which RFC 3834 §5.2 gives
     "messages generated by automatic (often periodic) processes"
     (<https://www.rfc-editor.org/rfc/rfc3834.html>).
   - multipart/alternative: text/plain, then text/html from the sanitised
     HTML path Matrix uses (ADR-0004), links to the web or mail only, URLs
     shown. No image, no remote resource, no style from event text.
   - Addresses (`from`, `reply_to` on the table; `to`, `cc`, `bcc` on a
     route) and display names come from the config alone, parsed as it
     loads; no event text reaches a header but the encoded subject.
   - One message per announcement, at most `max_bytes` (256 KiB where
     absent).
3. **Transports that carry the whole message.**
   - `"smtp"`: `host`; `tls = "implicit"` (the default, port 465) or
     `"starttls"` (port 587), `port` to change either. STARTTLS is
     required where named: a server that does not offer it is refused,
     herald never sends in the clear, and EHLO is sent again after TLS. A
     plain session is allowed only to a loopback host the config names,
     for fixtures (ADR-0007). `auth = "plain" | "login" | "xoauth2"` with
     `username` and `password = { secret }`; XOAUTH2 with
     `oauth = "microsoft" | "google"`, `tenant`, `client_id`,
     `client_secret = { secret }` and Microsoft's `cloud`.
   - `"graph"` (Microsoft 365, with `cloud` for each Graph cloud),
     `"gmail"`, `"ses"` (`region`, GovCloud among them) and `"mailgun"`
     (its US or EU host): the message as each takes it, over `Http` to the
     provider's documented host.
4. **Postmark and Resend are senders of a message's parts**, not of the
   message herald writes. herald gives each the subject, the text and
   HTML bodies, the addresses and its own headers, and the provider
   assembles the MIME message. What differs from the senders of herald's
   complete message:
   - the provider writes the multipart body, so herald's golden MIME is
     not what is sent; the parts are;
   - Resend threads on Resend's own Message-ID: after each send herald
     reads it back (`GET /emails/{id}`, one more request a part) and names
     it in the next part's `In-Reply-To` and `References`;
   - Postmark does not thread: each part is a message of its own, and
     each is sent with `TrackOpens` and `TrackLinks` false;
   - doctor names both differences for a route through either.
5. **A new port, `Streams`, opens an outbound TCP connection** to a host
   and port the destination names and nothing else, upgrades it to TLS in
   place or opens it with TLS, and verifies the certificate against the
   host. It carries bytes; the SMTP dialogue is sans-IO in
   `herald-dest-email`. `DestinationKind` gains `NEEDS`, as `StoreKind`
   has it, and `Offer` gains `Streams`.
6. **SMTP on every host that can open a connection.** `Offer::Streams` is
   on Actions, `herald server`, Lambda, Azure, Cloudflare Workers
   (`cloudflare:sockets` through the JavaScript shim), Node and Bun
   (`node:net` and `node:tls`), Deno (`Deno.connect` and `Deno.startTls`),
   Vercel and Netlify (Node.js functions). It is not on `wasi:http` hosts,
   whose world has no sockets, nor on Spin and Fastly, whose documentation
   names no outbound TCP for what herald runs there. Doctor says per host whether
   SMTP is offered, names port 25 where the host blocks it (Workers,
   Lambda, Azure, Vercel), says where a host documents no port rule (Deno
   Deploy, Netlify), and points a host without SMTP at the API transports,
   which run on every host.
7. **Privacy (ADR-0067).** herald cannot read who reads an address, so a
   private announcement goes to an email route only where the route says
   `private = true`, the operator's word, and every recipient of the route
   is written in the config. Doctor lists each such route as the
   operator's statement, not herald's check.
8. **Doctor `--online`.** SMTP: connect, TLS, EHLO, AUTH, QUIT, before any
   `MAIL FROM`, so nothing is sent. The API transports: the token or
   credential request alone.
9. **This extends** ADR-0001's ports with `Streams`, ADR-0024's host
   offers with `Offer::Streams`, and ADR-0004's dialects with email.

## Alternatives

| Option | Why not |
|--------|---------|
| **SMTP alone** | Hosts without sockets and teams on Microsoft 365, Gmail or SES would have no way in. |
| **An SMTP session as the port** | The protocol would be written once per host shim; a byte stream keeps it in one sans-IO crate. |
| **SMTP on the native binary first** | Workers, Node, Bun, Deno and Vercel open connections today; leaving them out would make the API transports the only way on hosts that need none. |
| **Postmark and Resend as transports** | Neither carries herald's own message, so threading and the body would not be herald's on them. |
| **Opportunistic STARTTLS** | A stripped `STARTTLS` would send the password and the message in the clear. |

## Consequences

- Email threads the same way on every transport herald offers.
- A host's SMTP reach is in `herald doctor --layer`, with the port rules
  its provider documents.
- Private announcements by email go only to addresses the operator wrote.

## Related

- [ADR-0001](0001-a-sans-io-core-behind-ports.md): ports; extended here
- [ADR-0004](0004-each-destination-gets-its-own-dialect-limits-and-features.md): dialects
- [ADR-0007](0007-secrets-stay-out-of-reach-and-every-inbound-request-is-verified-first.md): hosts a destination reaches
- [ADR-0024](0024-a-host-is-described-once-and-every-driver-composes-through-one-composition.md): what a host offers
- [ADR-0067](0067-a-view-shows-its-repositories-advisories-to-private-routes-alone.md): private places
