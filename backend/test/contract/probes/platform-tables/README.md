# Probe platform-tables

The fact: every table that the platform keeps for itself in an object's SQLite storage has a name
that starts with `_cf_`, except one table of the local runtime, `__miniflare_do_name`.

`wrangler dev` of wrangler 4.159.0 starts the local runtime, miniflare 5.20261109.0-beta, with the
option `unsafeInspectDurableObjects` set to true, and no flag, variable or config of wrangler turns
it off. With that option miniflare wraps every Durable Object class, and the wrapper's constructor,
for an object addressed by a name, makes the table `__miniflare_do_name` with the columns `id` and
`name` and writes one row: id 1 and the name. For an account object that name is the account id.
The vitest plugin does not set the option, so under vitest no wrapper runs and the table does not
exist.

The probe passes when every table but its own starts with `sqlite_` or `_cf_` or is
`__miniflare_do_name`, and when that table, if present, has the columns `id` and `name` and one row
whose name is the name by which the probe addressed its object. A table of any other name fails it.
The coordinator's ruling of 2026-11-10 on the failed probe exempts that one table by its exact name.
