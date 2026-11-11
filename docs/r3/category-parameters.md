# Category parameters of the agent-setup run

This file lists every parameter that an agent of the agent-setup run needs, one row per
seed category and parameter, for the five seed categories that have a task in this run.
The column `source` says where the value comes from: `activation` is the person's input,
choice or confirmation in the supervised first run, `provider_api` is read at run time
from a page on an origin of the agent's bindings or from the provider's API, `question`
is a guiding question, which no row has, and `open` is not settled in this run. The
column `bound in` says where the value is kept: `origins` is the list `origins` of the
record `agent_bindings`, `slot` is a slot of that record with the parameter's name, `run`
is read anew in each run and never stored, and `open` is not settled. The workflow run's
file `docs/r2/category-parameters.md` sends its reader here for the rows that it marks
`open`.

| category | parameter | source | bound in |
|---|---|---|---|
| utility_bill_payment | origin | activation | origins |
| utility_bill_payment | frame_origin | activation | origins |
| utility_bill_payment | start | activation | slot |
| utility_bill_payment | payee | activation | slot |
| utility_bill_payment | instrument | activation | slot |
| utility_bill_payment | amount_due | provider_api | run |
| email | account | activation | origins |
| email | reply_senders | activation | slot |
| email | messages | provider_api | run |
| spreadsheet_fill | workbook | activation | slot |
| spreadsheet_fill | sheet | activation | slot |
| spreadsheet_fill | range | activation | slot |
| spreadsheet_fill | source | activation | slot |
| shop_order | origin | activation | origins |
| shop_order | frame_origin | activation | origins |
| shop_order | start | activation | slot |
| shop_order | seller | activation | slot |
| shop_order | instrument | activation | slot |
| shop_order | items | provider_api | run |
| shop_order | total | provider_api | run |
| calendar_cleanup | account | activation | origins |
| calendar_cleanup | events | provider_api | run |

The five deferred categories, `tenant_rent`, `lease_renewal_or_change`, `tenant_application_review`, `ride_order` and `daily_chart_check`, have no row: their parameters are open.
