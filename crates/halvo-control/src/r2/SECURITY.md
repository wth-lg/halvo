# What a process of the same user can do with a request of protocol v2

The boundary of the control channel is in `crates/halvo-control/SECURITY.md`, with the rows of protocol v1. This file
holds the rows of the requests of protocol v2, in the form of that table.

| Request | What a process of the same user can do with it |
|---|---|
| `owner_stream` | Every program that runs as the person can speak to the capture service as the app does, and with protocol v2 it can ask for an end of the owner's channel in the role runner or coordinator. The server checks that the peer runs as the daemon's own user, as it does for protocol v1, and nothing else: the request names no role, and holding an end proves nothing about the holder. A check of the calling program by its signature waits for a signing identity (`PeerPolicy::SignedBy`). |
| `relay` | Every program that runs as the person can speak to the capture service as the app does, and with the request `relay` it acts in the role interface: it can read the found workflows with their evidence, approve a workflow, dismiss it and delete it. The daemon hands the owner of the derived store the object of the request and decides by nothing inside it; the owner takes the role from its channel to the daemon, which admits the role interface alone. The relay waits for no policy by signature: that check is the capture run's (`PeerPolicy::SignedBy`). |
