# Driver trial (golden case no_driver of the routing run's importer)

Invented values in the line forms of the agent-setup run's A13.6. No model ran.
The helper's start and end and the hosted runs stand here in a real trial file.

```
attempt model=m_tiny machine=hosted-win task=bill seed=3 kind=compile i=1 pass=no steps=9 wall_s=191 tokens_in=3507 tokens_out=343
attempt model=m_tiny machine=hosted-win task=bill seed=3 kind=replay i=1 pass=no steps=6 wall_s=11 tokens_in=0 tokens_out=0
attempt model=m_tiny machine=hosted-win task=bill seed=3 kind=replay i=2 pass=no steps=7 wall_s=12 tokens_in=0 tokens_out=0
attempt model=m_tiny machine=hosted-win task=bill seed=3 kind=replay i=3 pass=no steps=8 wall_s=13 tokens_in=0 tokens_out=0
attempt model=m_tiny machine=hosted-win task=bill seed=3 kind=recover i=1 pass=no steps=9 wall_s=191 tokens_in=3507 tokens_out=343
drive model=m_tiny machine=hosted-win task=bill seed=3 compile=miss replays_ok=0 of=3 recover=miss steps=9 wall_s=400 model_tokens=9000 typed_credential=0 effects_without_confirm=0
verdict driver=none path=B writer=none
```
