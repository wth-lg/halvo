# docs/r6: model routing

This directory holds the documents and data of the routing work, which picks for each call of an agent the cheapest model tier that meets the published quality bar and never passes the spend cap.

- `trials/` holds what the routing work adds to the model trials: the models it routes between and their lines for the trial.
- `gateway/` holds the contract for what a request asks of the model gateway, with its limits per tier and the vectors and sequences that both sides test against.
- `outcomes/` holds the recorded outcome rows that the routing table is derived from, with the sources they were read from.
- `owner/` holds the proposals that go to the owner for a decision.
- The files at the top of `docs/r6/` are single documents, each named for what it holds.

Every file under `docs/r6/` keeps LF line ends in every clone, because the tables, vectors and traces here are compared byte for byte.
