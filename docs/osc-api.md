# OSC operation API v1

Enable OSC in Preferences > Remote control. UDP uses the existing port (9000 by default).
TCP is independently enabled on the same numeric port. The listen address is installation-local,
defaults to `0.0.0.0`, and can be set to `127.0.0.1` for local-only operation. No project stores
network listener settings. This is a trusted local show-control network interface, without login.

Existing `/flux/play`, `/flux/pause`, `/flux/toggle`, `/flux/stop`, cue navigation, timeline
navigation, relative seek and view addresses retain their meaning. `/flux/go`, `/flux/devamp`
and `/flux/devamp/cancel` are additional fire-and-forget controls for simple buttons.

## Requests and replies

Every `/flux/v1/...` request starts with a nonempty OSC string request ID (up to 128 bytes).
Parameters below follow that ID. Replies go to the UDP sender's address/port or the same TCP
connection:

```text
/flux/v1/reply  <requestId:string> <ok:OSC-boolean> <result:JSON-string>
```

Successful actions return `{"accepted":true}`. This acknowledges queuing, not completion of a
transition; state feedback reports execution. Errors return `{"error":"reason"}`. An unknown
address, missing/invalid argument, unknown identity or unavailable output is an explicit error.

The last 256 replies are cached for 60 seconds by sender and request ID. Retrying an identical
request returns the cached reply without executing it again. Reusing an ID with different
content returns `requestIdConflict`. Give every new action or lease renewal a new ID. Legacy
fire-and-forget addresses do not have this deduplication guarantee.

Packets are OSC messages or bundles processed in order. Timetags are ignored, as with the
original interface: packets request immediate operation, not future scheduling. TCP packets
have a four-byte unsigned big-endian length prefix. Zero-length frames and frames larger than
65,507 bytes close the connection. Oversized replies return `responseTooLarge`.

## Dictionary

| Address | Additional arguments | Result/action |
| --- | --- | --- |
| `/flux/v1/state/get` | none | Revision and current show state |
| `/flux/v1/setlists/get` | none | Setlists and entries, including omitted entries |
| `/flux/v1/setlist/select` | setlist ID: string | Prepare the included entries; requires a stopped show |
| `/flux/v1/entry/prepare` | entry ID: string | Prepare an included entry in the current run; requires Stop |
| `/flux/v1/timelines/get` | none | Timeline IDs, names and timecode settings |
| `/flux/v1/timeline/select` | timeline ID: string | Leave setlist operation and prepare ordinary timeline playback; requires Stop |
| `/flux/v1/cues/get` | none | Current timeline's derived cues with stable IDs |
| `/flux/v1/cue/goto` | cue ID: string | Seek without replaying skipped events |
| `/flux/v1/go` | none | Run the displayed GO action |
| `/flux/v1/devamp` | none | Request exit after the current loop turn |
| `/flux/v1/devamp/cancel` | none | Withdraw the pending loop exit |
| `/flux/v1/transport/play` | none | Play/resume transport; explicit GO waits still require GO |
| `/flux/v1/transport/pause` | none | Pause |
| `/flux/v1/transport/stop` | none | Stop and reset the current entry |
| `/flux/v1/transport/seek` | absolute frame: nonnegative OSC int | Seek within the current timeline |
| `/flux/v1/tracks/get` | none | Current tracks and operational levels/mutes |
| `/flux/v1/track/gain` | track ID: string, dB: int/float | Set an audio/operator track's run level, −60 to +6; requires a prepared setlist |
| `/flux/v1/track/mute` | track ID: string, muted: bool or 0/1 | Set its run mute; requires a prepared setlist |
| `/flux/v1/outputs/get` | none | Output/device information and route health |
| `/flux/v1/subscribe` | optional Hz: int (1–30, default 10), optional UDP reply port: int | Subscribe or renew state feedback |
| `/flux/v1/unsubscribe` | none | Remove this sender's subscription |

Operational levels apply to every reference to that timeline in the prepared run. They do not
rewrite the project. Setlist order, controls and event editing remain in Edit, not in this API.

## Feedback

Feedback uses `/flux/v1/state` with the same three-argument reply envelope. Its JSON contains
`sequence`, `revision`, and `state`. Sequence increases per subscriber, so a UDP client can
detect missing updates. Query a fresh snapshot after a gap. Revision belongs to this listener's
lifetime and changes when its observed state changes; it is not a project revision.

The state includes entry/timeline identity, current and next entry, transport position and frame
rate, show status, the available GO action, loop turn, pending devamp, current/next cue,
running cues, audio route health and operational track levels. `go: "none"` means a GO button
should be disabled; `blocked` requires operator attention.

UDP leases expire after 30 seconds; renew with a new request ID before then. A reply port can
only redirect feedback to another port on the sender's IP. TCP subscriptions last until
unsubscribe or disconnection. Important state changes are sent promptly; position updates
follow the requested rate. There are at most 32 subscribers/connections, and slow TCP clients
are disconnected when their bounded pending data fills. Feedback cannot block audio playback.

## Examples

For a simple Stream Deck OSC button, send `/flux/go` without arguments. For a controller with
feedback, send these OSC messages (quotes denote OSC strings, not literal quote characters):

```text
/flux/v1/setlists/get "list-1"
/flux/v1/setlist/select "prepare-1" "<setlist-id-from-the-reply>"
/flux/v1/subscribe "subscribe-1" 10 9001
/flux/v1/go "go-1"
/flux/v1/devamp "exit-1"
/flux/v1/transport/stop "stop-1"
```

TouchOSC can listen on 9001 and use `state.status`, `state.go` and `state.devampPending` from
the feedback JSON to drive indicators. Renew its UDP subscription periodically with new IDs.
OSC packet delivery and external device behavior still need a sound/show-control check at the venue.
