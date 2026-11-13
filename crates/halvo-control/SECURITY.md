# Threat model of the control channel

This is the one text that says what a process can do through the control channel
between the Halvo app and the capture daemon. Later runs cite it and do not restate it.

## The boundary

The channel keeps other users out and nobody of the same user.

- macOS: the socket is `control.sock` under `run/` of the data directory, and `run/`
  has mode 0700. The server drops a peer whose user id differs from its own. Before
  it sends a request, the client requires the socket's directory to be a directory of
  its own user with mode 0700, the socket to be a socket of its own user, and the
  server to run as its own user.
- Windows: the pipe's access list grants the user's SID alone, the server drops a
  client whose token user is not its own user, and the client requires the token user
  of the server's process to equal its own. Until win-5 lands the pipe, the client on
  Windows answers Unsupported.
- The app can also require the server to be the child that it started, by its process
  id. `ctl` cannot: it talks to whichever daemon holds the data directory.

Until builds are signed, any process of the same user can read whatever a reply or a section carries and can send every request, a later run's included.

## What a process of the same user can do, request by request

| Request | What a process of the same user can do with it |
|---|---|
| `status` | Read the status: capture and store state, counts of events and losses, the store id, the daemon's pid and version, and the state of each permission. With `indicator: true` it renews the indicator lease, so a fresh lease proves that a client sends the claim, not that an indicator is on screen. |
| `inventory` | Read the activity volume by day and kind: events and bytes, and the store's disk bytes. |
| `pause` | Pause capture; the state file then says paused. |
| `resume` | Resume capture; the sources start only while the indicator lease is fresh. |
| `stop` | Stop the daemon: it flushes, replies and exits. |
| `delete_all` | Delete everything that the store holds, with the store id that the status shows, or with no id when the status shows none. |
| `set_storage_cap` | Lower the cap to its minimum of 1 GiB, which evicts the oldest data, or raise it to 1 TiB. |
| `request_permission` | Make the daemon start the permission request child, which can raise a request of macOS. |
| `permission_asked` | Record that a permission was asked about. The daemon of R1 answers it `unsupported_op`. |
| `acknowledge_lost_permission` | Clear the notice that a permission was lost. |
| `run_start` | Nothing in R1: the name is reserved, and protocol v1 answers it `unsupported_op`. |
| `run_end` | Nothing in R1: the name is reserved, and protocol v1 answers it `unsupported_op`. |

Every other op is answered `unsupported_op`. In the key `v` of a frame that is one JSON
object, a version is an integer from 0 to 18446744073709551615. A frame that starts with `{`
and ends with `}`, is one JSON object with each key once at every depth, and holds in `v` a
version other than 1 is answered `unsupported_version`; a `v` that is no such integer is
answered `invalid_argument`. Expiry, cap eviction and delete-all need no lease;
after a lapse capture reads `no_indicator`, and the lapse writes nothing to the state
file.

One decode of a request frame of at most 4,096 bytes holds at most 524,288 bytes live at
once (`REQUEST_DECODE_PEAK_BYTES`), and the worst shape that the crate's test measures,
objects of the empty key nested side by side, holds 471,098. One decode of a reply frame of
at most 262,144 bytes holds at most 33,554,432 bytes live at once (`REPLY_DECODE_PEAK_BYTES`),
and the same shape holds 33,009,946.

## What never crosses the channel

No request or reply carries an app id, a window title, a site, a segment id or a
segment path. No field of a reply is opened or run: the settings pane that the app
opens comes from its own table, keyed by the typed permission. No R1 code reads a
descriptor or a handle from a reply.

## What the same user can do without the channel

- Read and change the files of the data directory.
- Send signals to the daemon.
- Run the offline delete, `halvo-capture --delete-all`.
- Answer as a false server on the socket path before the daemon binds it. The app's
  guard against that is the pid of the child it started; `ctl` has none.
- Unwrap the root. On Windows DPAPI's user scope lets any program of the same user
  unprotect the root file. The root file of an unsigned macOS build has owner-only
  permissions and is readable by the same user.
- Reach the page of the app, for example through a debugging interface of the web
  view, and invoke every command that the app's capability allows, a later run's
  included. On Windows the app clears WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS before it
  makes the web view.

With the feature `r2`, which no R1 build turns on, and until builds are signed, a process of the same user can get an end of the owner stream and send on it runner and coordinator_write requests that a handler commits, and a false server can hand the app an end of a stream that it serves itself and read what the app sends; holding an end proves nothing about the holder, and the app's guard against a false server is the pid of the child that it started.

## Rules for later runs

- No approval for money or for signing rests on this channel alone.
- A later run's read channel is a stream that the daemon hands to a child it starts,
  never a listening endpoint.
- A request that changes daemon state falls under the peer policy.

## The control that waits for signing

`PeerPolicy` in `proto.rs` has one value that R1 can build, `SameUser`. Its second
value, `SignedBy`, holds a signing identity, a type with no value until the owner
gives one; then the peers' code signatures can be checked as well.

## Rows of a later run

A later run that adds requests puts the rows of its own requests in
`crates/halvo-control/src/r2/SECURITY.md`, beside its code, in the form of the table
above, with its words about proposals and workflows. This file keeps the rows of
protocol v1.
