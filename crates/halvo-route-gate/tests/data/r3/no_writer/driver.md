# Driver trial (golden case no_writer of the routing run's importer)

Invented values in the line forms of the agent-setup run's A13.6. No model ran.
The helper's start and end and the hosted runs stand here in a real trial file.

```text
attempt model=m_small machine=mac task=bill seed=1 kind=compile i=1 pass=yes steps=4 wall_s=162 tokens_in=3644 tokens_out=366
attempt model=m_small machine=mac task=bill seed=1 kind=replay i=1 pass=yes steps=7 wall_s=12 tokens_in=0 tokens_out=0
attempt model=m_small machine=mac task=bill seed=1 kind=replay i=2 pass=yes steps=8 wall_s=13 tokens_in=0 tokens_out=0
attempt model=m_small machine=mac task=bill seed=1 kind=replay i=3 pass=yes steps=9 wall_s=14 tokens_in=0 tokens_out=0

attempt model=m_small machine=mac task=bill seed=1 kind=recover i=1 pass=yes steps=4 wall_s=162 tokens_in=3644 tokens_out=366
attempt model=m_small machine=mac task=bill seed=2 kind=compile i=1 pass=yes steps=4 wall_s=30 tokens_in=2000 tokens_out=90
attempt model=m_small machine=mac task=bill seed=2 kind=replay i=1 pass=yes steps=6 wall_s=17 tokens_in=0 tokens_out=0
attempt model=m_small machine=mac task=bill seed=2 kind=replay i=2 pass=yes steps=7 wall_s=18 tokens_in=0 tokens_out=0
attempt model=m_small machine=mac task=bill seed=2 kind=replay i=3 pass=yes steps=8 wall_s=19 tokens_in=0 tokens_out=0
attempt model=m_small machine=mac task=bill seed=2 kind=recover i=1 pass=yes steps=4 wall_s=30 tokens_in=2000 tokens_out=90
attempt model=m_small machine=mac task=bill seed=3 kind=compile i=1 pass=yes steps=9 wall_s=85 tokens_in=2685 tokens_out=205
notrun model=m_small machine=mac task=bill seed=1 cause=browser_start
attempt model=m_small machine=mac task=bill seed=3 kind=replay i=1 pass=yes steps=6 wall_s=5 tokens_in=0 tokens_out=0
attempt model=m_small machine=mac task=bill seed=3 kind=replay i=2 pass=yes steps=7 wall_s=6 tokens_in=0 tokens_out=0
attempt model=m_small machine=mac task=bill seed=3 kind=replay i=3 pass=yes steps=8 wall_s=7 tokens_in=0 tokens_out=0
attempt model=m_small machine=mac task=bill seed=3 kind=recover i=1 pass=yes steps=9 wall_s=85 tokens_in=2685 tokens_out=205
attempt model=m_small machine=mac task=order seed=1 kind=compile i=1 pass=yes steps=5 wall_s=173 tokens_in=3781 tokens_out=389
attempt model=m_small machine=mac task=order seed=1 kind=replay i=1 pass=yes steps=8 wall_s=13 tokens_in=0 tokens_out=0
attempt model=m_small machine=mac task=order seed=1 kind=replay i=2 pass=yes steps=9 wall_s=14 tokens_in=0 tokens_out=0
attempts model=m_small machine=mac
attempt model=m_small machine=mac task=order seed=1 kind=replay i=3 pass=yes steps=4 wall_s=15 tokens_in=0 tokens_out=0
attempt model=m_small machine=mac task=order seed=1 kind=recover i=1 pass=yes steps=5 wall_s=173 tokens_in=3781 tokens_out=389
attempt model=m_small machine=mac task=order seed=2 kind=compile i=1 pass=yes steps=5 wall_s=41 tokens_in=2137 tokens_out=113
attempt model=m_small machine=mac task=order seed=2 kind=replay i=1 pass=yes steps=7 wall_s=18 tokens_in=0 tokens_out=0
attempt model=m_small machine=mac task=order seed=2 kind=replay i=2 pass=yes steps=8 wall_s=19 tokens_in=0 tokens_out=0
attempt model=m_small machine=mac task=order seed=2 kind=replay i=3 pass=yes steps=4 wall_s=3 tokens_in=0 tokens_out=0
attempt model=m_small machine=mac task=order seed=2 kind=recover i=1 pass=yes steps=5 wall_s=41 tokens_in=2137 tokens_out=113
attempt_model=m_small
attempt model=m_small machine=mac task=order seed=3 kind=compile i=1 pass=yes steps=4 wall_s=96 tokens_in=2822 tokens_out=228
attempt model=m_small machine=mac task=order seed=3 kind=replay i=1 pass=yes steps=7 wall_s=6 tokens_in=0 tokens_out=0
attempt model=m_small machine=mac task=order seed=3 kind=replay i=2 pass=yes steps=8 wall_s=7 tokens_in=0 tokens_out=0
attempt model=m_small machine=mac task=order seed=3 kind=replay i=3 pass=yes steps=9 wall_s=8 tokens_in=0 tokens_out=0
attempt model=m_small machine=mac task=order seed=3 kind=recover i=1 pass=yes steps=4 wall_s=96 tokens_in=2822 tokens_out=228
attempt model=m_small machine=mac task=reply seed=1 kind=write i=1 pass=yes steps=0 wall_s=107 tokens_in=2959 tokens_out=251
attempt model=m_small machine=mac task=reply seed=2 kind=write i=1 pass=yes steps=0 wall_s=162 tokens_in=3644 tokens_out=366
attempt
attempt model=m_small machine=mac task=reply seed=3 kind=write i=1 pass=yes steps=0 wall_s=30 tokens_in=2000 tokens_out=90
drive model=m_small machine=mac task=bill seed=1 compile=ok replays_ok=3 of=3 recover=ok steps=9 wall_s=400 model_tokens=9000 typed_credential=0 effects_without_confirm=0
drive model=m_small machine=mac task=bill seed=2 compile=ok replays_ok=3 of=3 recover=ok steps=9 wall_s=400 model_tokens=9000 typed_credential=0 effects_without_confirm=0
drive model=m_small machine=mac task=bill seed=3 compile=ok replays_ok=3 of=3 recover=ok steps=9 wall_s=400 model_tokens=9000 typed_credential=0 effects_without_confirm=0
drive model=m_small machine=mac task=order seed=1 compile=ok replays_ok=3 of=3 recover=ok steps=9 wall_s=400 model_tokens=9000 typed_credential=0 effects_without_confirm=0
drive model=m_small machine=mac task=order seed=2 compile=ok replays_ok=3 of=3 recover=ok steps=9 wall_s=400 model_tokens=9000 typed_credential=0 effects_without_confirm=0
drive model=m_small machine=mac task=order seed=3 compile=ok replays_ok=3 of=3 recover=ok steps=9 wall_s=400 model_tokens=9000 typed_credential=0 effects_without_confirm=0
 attempt model=m_small machine=mac task=bill seed=1 kind=compile i=9 pass=no steps=1 wall_s=1 tokens_in=1 tokens_out=1
reply model=m_small machine=mac seed=1 bodies=2 of=2 wall_s=120 model_tokens=700
reply model=m_small machine=mac seed=2 bodies=2 of=2 wall_s=120 model_tokens=700
reply model=m_small machine=mac seed=3 bodies=2 of=2 wall_s=120 model_tokens=700
attempt model=m_small machine=hosted-win task=bill seed=1 kind=compile i=1 pass=yes steps=6 wall_s=158 tokens_in=3096 tokens_out=274
attempt model=m_small machine=hosted-win task=bill seed=1 kind=replay i=1 pass=yes steps=9 wall_s=8 tokens_in=0 tokens_out=0
attempt model=m_small machine=hosted-win task=bill seed=1 kind=replay i=2 pass=yes steps=4 wall_s=9 tokens_in=0 tokens_out=0
attempt model=m_small machine=hosted-win task=bill seed=1 kind=replay i=3 pass=yes steps=5 wall_s=10 tokens_in=0 tokens_out=0
verdict candidates=-
attempt model=m_small machine=hosted-win task=bill seed=1 kind=recover i=1 pass=yes steps=6 wall_s=158 tokens_in=3096 tokens_out=274
attempt model=m_small machine=hosted-win task=bill seed=2 kind=compile i=1 pass=yes steps=5 wall_s=213 tokens_in=3781 tokens_out=389
attempt model=m_small machine=hosted-win task=bill seed=2 kind=replay i=1 pass=yes steps=8 wall_s=13 tokens_in=0 tokens_out=0
attempt model=m_small machine=hosted-win task=bill seed=2 kind=replay i=2 pass=yes steps=9 wall_s=14 tokens_in=0 tokens_out=0
attempt model=m_small machine=hosted-win task=bill seed=2 kind=replay i=3 pass=yes steps=4 wall_s=15 tokens_in=0 tokens_out=0
attempt model=m_small machine=hosted-win task=bill seed=2 kind=recover i=1 pass=yes steps=5 wall_s=213 tokens_in=3781 tokens_out=389
attempt model=m_small machine=hosted-win task=bill seed=3 kind=compile i=1 pass=yes steps=5 wall_s=81 tokens_in=2137 tokens_out=113
verdict  driver=m_tiny path=A writer=m_tiny
attempt model=m_small machine=hosted-win task=bill seed=3 kind=replay i=1 pass=yes steps=7 wall_s=18 tokens_in=0 tokens_out=0
attempt model=m_small machine=hosted-win task=bill seed=3 kind=replay i=2 pass=yes steps=8 wall_s=19 tokens_in=0 tokens_out=0
attempt model=m_small machine=hosted-win task=bill seed=3 kind=replay i=3 pass=yes steps=4 wall_s=3 tokens_in=0 tokens_out=0
attempt model=m_small machine=hosted-win task=bill seed=3 kind=recover i=1 pass=yes steps=5 wall_s=81 tokens_in=2137 tokens_out=113
attempt model=m_small machine=hosted-win task=order seed=1 kind=compile i=1 pass=yes steps=7 wall_s=169 tokens_in=3233 tokens_out=297
attempt model=m_small machine=hosted-win task=order seed=1 kind=replay i=1 pass=yes steps=4 wall_s=9 tokens_in=0 tokens_out=0
attempt model=m_small machine=hosted-win task=order seed=1 kind=replay i=2 pass=yes steps=5 wall_s=10 tokens_in=0 tokens_out=0
bounds model=m_small
attempt model=m_small machine=hosted-win task=order seed=1 kind=replay i=3 pass=yes steps=6 wall_s=11 tokens_in=0 tokens_out=0
attempt model=m_small machine=hosted-win task=order seed=1 kind=recover i=1 pass=yes steps=7 wall_s=169 tokens_in=3233 tokens_out=297
attempt model=m_small machine=hosted-win task=order seed=2 kind=compile i=1 pass=yes steps=6 wall_s=224 tokens_in=3918 tokens_out=412
attempt model=m_small machine=hosted-win task=order seed=2 kind=replay i=1 pass=yes steps=9 wall_s=14 tokens_in=0 tokens_out=0
attempt model=m_small machine=hosted-win task=order seed=2 kind=replay i=2 pass=yes steps=4 wall_s=15 tokens_in=0 tokens_out=0
attempt model=m_small machine=hosted-win task=order seed=2 kind=replay i=3 pass=yes steps=5 wall_s=16 tokens_in=0 tokens_out=0
attempt model=m_small machine=hosted-win task=order seed=2 kind=recover i=1 pass=yes steps=6 wall_s=224 tokens_in=3918 tokens_out=412
```text
attempt model=m_small machine=hosted-win task=order seed=3 kind=compile i=1 pass=yes steps=6 wall_s=92 tokens_in=2274 tokens_out=136
attempt model=m_small machine=hosted-win task=order seed=3 kind=replay i=1 pass=yes steps=8 wall_s=19 tokens_in=0 tokens_out=0
attempt model=m_small machine=hosted-win task=order seed=3 kind=replay i=2 pass=yes steps=4 wall_s=3 tokens_in=0 tokens_out=0
attempt model=m_small machine=hosted-win task=order seed=3 kind=replay i=3 pass=yes steps=5 wall_s=4 tokens_in=0 tokens_out=0
attempt model=m_small machine=hosted-win task=order seed=3 kind=recover i=1 pass=yes steps=6 wall_s=92 tokens_in=2274 tokens_out=136
attempt model=m_small machine=hosted-win task=reply seed=1 kind=write i=1 pass=yes steps=0 wall_s=103 tokens_in=2411 tokens_out=159
attempt model=m_small machine=hosted-win task=reply seed=2 kind=write i=1 pass=yes steps=0 wall_s=158 tokens_in=3096 tokens_out=274
``
attempt model=m_small machine=hosted-win task=reply seed=3 kind=write i=1 pass=no steps=0 wall_s=213 tokens_in=3781 tokens_out=389
drive model=m_small machine=hosted-win task=bill seed=1 compile=ok replays_ok=3 of=3 recover=ok steps=9 wall_s=400 model_tokens=9000 typed_credential=0 effects_without_confirm=0
drive model=m_small machine=hosted-win task=bill seed=2 compile=ok replays_ok=3 of=3 recover=ok steps=9 wall_s=400 model_tokens=9000 typed_credential=0 effects_without_confirm=0
drive model=m_small machine=hosted-win task=bill seed=3 compile=ok replays_ok=3 of=3 recover=ok steps=9 wall_s=400 model_tokens=9000 typed_credential=0 effects_without_confirm=0
drive model=m_small machine=hosted-win task=order seed=1 compile=ok replays_ok=3 of=3 recover=ok steps=9 wall_s=400 model_tokens=9000 typed_credential=0 effects_without_confirm=0
drive model=m_small machine=hosted-win task=order seed=2 compile=ok replays_ok=3 of=3 recover=ok steps=9 wall_s=400 model_tokens=9000 typed_credential=0 effects_without_confirm=0
drive model=m_small machine=hosted-win task=order seed=3 compile=ok replays_ok=3 of=3 recover=ok steps=9 wall_s=400 model_tokens=9000 typed_credential=0 effects_without_confirm=0
````
reply model=m_small machine=hosted-win seed=1 bodies=2 of=2 wall_s=120 model_tokens=700
reply model=m_small machine=hosted-win seed=2 bodies=2 of=2 wall_s=120 model_tokens=700
reply model=m_small machine=hosted-win seed=3 bodies=1 of=2 wall_s=120 model_tokens=700
attempt model=m_tiny machine=mac task=bill seed=1 kind=compile i=1 pass=yes steps=9 wall_s=85 tokens_in=2685 tokens_out=205
attempt model=m_tiny machine=mac task=bill seed=1 kind=replay i=1 pass=yes steps=6 wall_s=5 tokens_in=0 tokens_out=0
attempt model=m_tiny machine=mac task=bill seed=1 kind=replay i=2 pass=yes steps=7 wall_s=6 tokens_in=0 tokens_out=0
attempt model=m_tiny machine=mac task=bill seed=1 kind=replay i=3 pass=yes steps=8 wall_s=7 tokens_in=0 tokens_out=0

attempt model=m_tiny machine=mac task=bill seed=1 kind=recover i=1 pass=yes steps=9 wall_s=85 tokens_in=2685 tokens_out=205
attempt model=m_tiny machine=mac task=bill seed=2 kind=compile i=1 pass=yes steps=8 wall_s=140 tokens_in=3370 tokens_out=320
attempt model=m_tiny machine=mac task=bill seed=2 kind=replay i=1 pass=yes steps=5 wall_s=10 tokens_in=0 tokens_out=0
attempt model=m_tiny machine=mac task=bill seed=2 kind=replay i=2 pass=yes steps=6 wall_s=11 tokens_in=0 tokens_out=0
attempt model=m_tiny machine=mac task=bill seed=2 kind=replay i=3 pass=yes steps=7 wall_s=12 tokens_in=0 tokens_out=0
attempt model=m_tiny machine=mac task=bill seed=2 kind=recover i=1 pass=yes steps=8 wall_s=140 tokens_in=3370 tokens_out=320
attempt model=m_tiny machine=mac task=bill seed=3 kind=compile i=1 pass=yes steps=7 wall_s=195 tokens_in=4055 tokens_out=435
notrun model=m_small machine=mac task=bill seed=1 cause=browser_start
attempt model=m_tiny machine=mac task=bill seed=3 kind=replay i=1 pass=yes steps=4 wall_s=15 tokens_in=0 tokens_out=0
attempt model=m_tiny machine=mac task=bill seed=3 kind=replay i=2 pass=yes steps=5 wall_s=16 tokens_in=0 tokens_out=0
attempt model=m_tiny machine=mac task=bill seed=3 kind=replay i=3 pass=yes steps=6 wall_s=17 tokens_in=0 tokens_out=0
attempt model=m_tiny machine=mac task=bill seed=3 kind=recover i=1 pass=yes steps=7 wall_s=195 tokens_in=4055 tokens_out=435
attempt model=m_tiny machine=mac task=order seed=1 kind=compile i=1 pass=yes steps=4 wall_s=96 tokens_in=2822 tokens_out=228
attempt model=m_tiny machine=mac task=order seed=1 kind=replay i=1 pass=yes steps=7 wall_s=6 tokens_in=0 tokens_out=0
attempt model=m_tiny machine=mac task=order seed=1 kind=replay i=2 pass=yes steps=8 wall_s=7 tokens_in=0 tokens_out=0
attempts model=m_small machine=mac
attempt model=m_tiny machine=mac task=order seed=1 kind=replay i=3 pass=yes steps=9 wall_s=8 tokens_in=0 tokens_out=0
attempt model=m_tiny machine=mac task=order seed=1 kind=recover i=1 pass=yes steps=4 wall_s=96 tokens_in=2822 tokens_out=228
attempt model=m_tiny machine=mac task=order seed=2 kind=compile i=1 pass=yes steps=9 wall_s=151 tokens_in=3507 tokens_out=343
attempt model=m_tiny machine=mac task=order seed=2 kind=replay i=1 pass=yes steps=6 wall_s=11 tokens_in=0 tokens_out=0
attempt model=m_tiny machine=mac task=order seed=2 kind=replay i=2 pass=yes steps=7 wall_s=12 tokens_in=0 tokens_out=0
attempt model=m_tiny machine=mac task=order seed=2 kind=replay i=3 pass=yes steps=8 wall_s=13 tokens_in=0 tokens_out=0
attempt model=m_tiny machine=mac task=order seed=2 kind=recover i=1 pass=yes steps=9 wall_s=151 tokens_in=3507 tokens_out=343
attempt_model=m_small
attempt model=m_tiny machine=mac task=order seed=3 kind=compile i=1 pass=yes steps=8 wall_s=206 tokens_in=4192 tokens_out=458
attempt model=m_tiny machine=mac task=order seed=3 kind=replay i=1 pass=yes steps=5 wall_s=16 tokens_in=0 tokens_out=0
attempt model=m_tiny machine=mac task=order seed=3 kind=replay i=2 pass=yes steps=6 wall_s=17 tokens_in=0 tokens_out=0
attempt model=m_tiny machine=mac task=order seed=3 kind=replay i=3 pass=yes steps=7 wall_s=18 tokens_in=0 tokens_out=0
attempt model=m_tiny machine=mac task=order seed=3 kind=recover i=1 pass=yes steps=8 wall_s=206 tokens_in=4192 tokens_out=458
attempt model=m_tiny machine=mac task=reply seed=1 kind=write i=1 pass=yes steps=0 wall_s=30 tokens_in=2000 tokens_out=90
attempt model=m_tiny machine=mac task=reply seed=2 kind=write i=1 pass=yes steps=0 wall_s=85 tokens_in=2685 tokens_out=205
attempt
attempt model=m_tiny machine=mac task=reply seed=3 kind=write i=1 pass=yes steps=0 wall_s=140 tokens_in=3370 tokens_out=320
drive model=m_tiny machine=mac task=bill seed=1 compile=ok replays_ok=3 of=3 recover=ok steps=9 wall_s=400 model_tokens=9000 typed_credential=0 effects_without_confirm=0
drive model=m_tiny machine=mac task=bill seed=2 compile=ok replays_ok=3 of=3 recover=ok steps=9 wall_s=400 model_tokens=9000 typed_credential=0 effects_without_confirm=0
drive model=m_tiny machine=mac task=bill seed=3 compile=ok replays_ok=3 of=3 recover=ok steps=9 wall_s=400 model_tokens=9000 typed_credential=0 effects_without_confirm=0
drive model=m_tiny machine=mac task=order seed=1 compile=ok replays_ok=3 of=3 recover=ok steps=9 wall_s=400 model_tokens=9000 typed_credential=0 effects_without_confirm=0
drive model=m_tiny machine=mac task=order seed=2 compile=ok replays_ok=3 of=3 recover=ok steps=9 wall_s=400 model_tokens=9000 typed_credential=0 effects_without_confirm=0
drive model=m_tiny machine=mac task=order seed=3 compile=ok replays_ok=3 of=3 recover=ok steps=9 wall_s=400 model_tokens=9000 typed_credential=0 effects_without_confirm=0
 attempt model=m_small machine=mac task=bill seed=1 kind=compile i=9 pass=no steps=1 wall_s=1 tokens_in=1 tokens_out=1
reply model=m_tiny machine=mac seed=1 bodies=2 of=2 wall_s=120 model_tokens=700
reply model=m_tiny machine=mac seed=2 bodies=2 of=2 wall_s=120 model_tokens=700
reply model=m_tiny machine=mac seed=3 bodies=2 of=2 wall_s=120 model_tokens=700
attempt model=m_tiny machine=hosted-win task=bill seed=1 kind=compile i=1 pass=yes steps=5 wall_s=81 tokens_in=2137 tokens_out=113
attempt model=m_tiny machine=hosted-win task=bill seed=1 kind=replay i=1 pass=yes steps=7 wall_s=18 tokens_in=0 tokens_out=0
attempt model=m_tiny machine=hosted-win task=bill seed=1 kind=replay i=2 pass=yes steps=8 wall_s=19 tokens_in=0 tokens_out=0
attempt model=m_tiny machine=hosted-win task=bill seed=1 kind=replay i=3 pass=yes steps=4 wall_s=3 tokens_in=0 tokens_out=0
verdict candidates=-
attempt model=m_tiny machine=hosted-win task=bill seed=1 kind=recover i=1 pass=yes steps=5 wall_s=81 tokens_in=2137 tokens_out=113
attempt model=m_tiny machine=hosted-win task=bill seed=2 kind=compile i=1 pass=yes steps=4 wall_s=136 tokens_in=2822 tokens_out=228
attempt model=m_tiny machine=hosted-win task=bill seed=2 kind=replay i=1 pass=yes steps=7 wall_s=6 tokens_in=0 tokens_out=0
attempt model=m_tiny machine=hosted-win task=bill seed=2 kind=replay i=2 pass=yes steps=8 wall_s=7 tokens_in=0 tokens_out=0
attempt model=m_tiny machine=hosted-win task=bill seed=2 kind=replay i=3 pass=yes steps=9 wall_s=8 tokens_in=0 tokens_out=0
attempt model=m_tiny machine=hosted-win task=bill seed=2 kind=recover i=1 pass=yes steps=4 wall_s=136 tokens_in=2822 tokens_out=228
attempt model=m_tiny machine=hosted-win task=bill seed=3 kind=compile i=1 pass=yes steps=9 wall_s=191 tokens_in=3507 tokens_out=343
verdict  driver=m_tiny path=A writer=m_tiny
attempt model=m_tiny machine=hosted-win task=bill seed=3 kind=replay i=1 pass=yes steps=6 wall_s=11 tokens_in=0 tokens_out=0
attempt model=m_tiny machine=hosted-win task=bill seed=3 kind=replay i=2 pass=yes steps=7 wall_s=12 tokens_in=0 tokens_out=0
attempt model=m_tiny machine=hosted-win task=bill seed=3 kind=replay i=3 pass=yes steps=8 wall_s=13 tokens_in=0 tokens_out=0
attempt model=m_tiny machine=hosted-win task=bill seed=3 kind=recover i=1 pass=yes steps=9 wall_s=191 tokens_in=3507 tokens_out=343
attempt model=m_tiny machine=hosted-win task=order seed=1 kind=compile i=1 pass=yes steps=6 wall_s=92 tokens_in=2274 tokens_out=136
attempt model=m_tiny machine=hosted-win task=order seed=1 kind=replay i=1 pass=yes steps=8 wall_s=19 tokens_in=0 tokens_out=0
attempt model=m_tiny machine=hosted-win task=order seed=1 kind=replay i=2 pass=yes steps=4 wall_s=3 tokens_in=0 tokens_out=0
bounds model=m_small
attempt model=m_tiny machine=hosted-win task=order seed=1 kind=replay i=3 pass=yes steps=5 wall_s=4 tokens_in=0 tokens_out=0
attempt model=m_tiny machine=hosted-win task=order seed=1 kind=recover i=1 pass=yes steps=6 wall_s=92 tokens_in=2274 tokens_out=136
attempt model=m_tiny machine=hosted-win task=order seed=2 kind=compile i=1 pass=no steps=5 wall_s=147 tokens_in=2959 tokens_out=251
attempt model=m_tiny machine=hosted-win task=order seed=2 kind=replay i=1 pass=yes steps=8 wall_s=7 tokens_in=0 tokens_out=0
attempt model=m_tiny machine=hosted-win task=order seed=2 kind=replay i=2 pass=yes steps=9 wall_s=8 tokens_in=0 tokens_out=0
attempt model=m_tiny machine=hosted-win task=order seed=2 kind=replay i=3 pass=yes steps=4 wall_s=9 tokens_in=0 tokens_out=0
attempt model=m_tiny machine=hosted-win task=order seed=2 kind=recover i=1 pass=yes steps=5 wall_s=147 tokens_in=2959 tokens_out=251
```text
attempt model=m_tiny machine=hosted-win task=order seed=3 kind=compile i=1 pass=yes steps=4 wall_s=202 tokens_in=3644 tokens_out=366
attempt model=m_tiny machine=hosted-win task=order seed=3 kind=replay i=1 pass=yes steps=7 wall_s=12 tokens_in=0 tokens_out=0
attempt model=m_tiny machine=hosted-win task=order seed=3 kind=replay i=2 pass=yes steps=8 wall_s=13 tokens_in=0 tokens_out=0
attempt model=m_tiny machine=hosted-win task=order seed=3 kind=replay i=3 pass=yes steps=9 wall_s=14 tokens_in=0 tokens_out=0
attempt model=m_tiny machine=hosted-win task=order seed=3 kind=recover i=1 pass=yes steps=4 wall_s=202 tokens_in=3644 tokens_out=366
attempt model=m_tiny machine=hosted-win task=reply seed=1 kind=write i=1 pass=yes steps=0 wall_s=213 tokens_in=3781 tokens_out=389
attempt model=m_tiny machine=hosted-win task=reply seed=2 kind=write i=1 pass=yes steps=0 wall_s=81 tokens_in=2137 tokens_out=113
``
attempt model=m_tiny machine=hosted-win task=reply seed=3 kind=write i=1 pass=yes steps=0 wall_s=136 tokens_in=2822 tokens_out=228
drive model=m_tiny machine=hosted-win task=bill seed=1 compile=ok replays_ok=3 of=3 recover=ok steps=9 wall_s=400 model_tokens=9000 typed_credential=0 effects_without_confirm=0
drive model=m_tiny machine=hosted-win task=bill seed=2 compile=ok replays_ok=3 of=3 recover=ok steps=9 wall_s=400 model_tokens=9000 typed_credential=0 effects_without_confirm=0
drive model=m_tiny machine=hosted-win task=bill seed=3 compile=ok replays_ok=3 of=3 recover=ok steps=9 wall_s=400 model_tokens=9000 typed_credential=0 effects_without_confirm=0
drive model=m_tiny machine=hosted-win task=order seed=1 compile=ok replays_ok=3 of=3 recover=ok steps=9 wall_s=400 model_tokens=9000 typed_credential=0 effects_without_confirm=0
drive model=m_tiny machine=hosted-win task=order seed=2 compile=miss replays_ok=3 of=3 recover=ok steps=9 wall_s=400 model_tokens=9000 typed_credential=0 effects_without_confirm=0
drive model=m_tiny machine=hosted-win task=order seed=3 compile=ok replays_ok=3 of=3 recover=ok steps=9 wall_s=400 model_tokens=9000 typed_credential=0 effects_without_confirm=0
````
reply model=m_tiny machine=hosted-win seed=1 bodies=2 of=2 wall_s=120 model_tokens=700
reply model=m_tiny machine=hosted-win seed=2 bodies=2 of=2 wall_s=120 model_tokens=700
reply model=m_tiny machine=hosted-win seed=3 bodies=2 of=2 wall_s=120 model_tokens=700
limits task=bill steps=9 wall_s=400 model_tokens=9000
limits task=order steps=9 wall_s=400 model_tokens=9000
limits task=reply wall_s=120 model_tokens=700
prompt file=action.txt sha256=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
prompt file=reply.txt sha256=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
hosted machine=hosted-win run=1234567890 sha=cccccccccccccccccccccccccccccccccccccccc
verdict driver=m_small path=A writer=none
```
