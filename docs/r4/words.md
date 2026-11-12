# Glossary of the words that Halvo shows

This glossary gives each word that the window shows to a person one meaning and one owning run. It is item I1.6 of the interface run's contract. The check `scripts/r4/ci.d/45-r4-words.sh` reads the table below.

How the check uses the table:

- A term with `yes` in the column reserved is reserved. The check matches it as a whole word in any letter case, in the exact form of its row. So `paused` matches `Paused` and does not match `pause`.
- A sentence of the interface run fails the rule `clash` when it holds a reserved term of another run.
- The interface run uses one reserved term of another run: the text of the control `delete`. The file `scripts/r4/words/allowed.txt` holds that one pair of a words key and a term.

A row for a further term of another run comes by a request of that run's planner. Write the term on its own, with no code font, because the check reads the cell as it stands.

| term | meaning | run | reserved |
|---|---|---|---|
| paused | The capture run's word for capture that is stopped. | capture | yes |
| cap | The capture run's limit on storage, and the billing run's limit on spend. The interface run shows neither. | capture | yes |
| delete | The person removes a stored workflow. The Delete control of an agent sends the workflow run's delete command. | workflow | yes |
| approve | The person accepts a proposal in the workflow run's list. | workflow | yes |
| agent | A helper that does one kind of task for the person, such as paying a utility bill on one site. | interface | no |
| task | Two senses: the kind of work that an agent does for the person, and one time that it does that work, as in its last task. | interface | no |
| Needs you | The status of an agent that waits for the person to answer or to act. | interface | yes |
| Problem | The status of an agent whose last task failed, or that Halvo switched off after it failed several times in a row. | interface | no |
| Working | The status of an agent that is doing its task now. | interface | no |
| Waiting its turn | The status of an agent that waits to start, because only one agent works at a time. | interface | yes |
| All set | The status of an agent that needs nothing from the person. | interface | no |
| Off | The status of an agent that the person switched off. It does not start until the person switches it on. | interface | no |
| switched off | Said of an agent that does not start on its own: the person switched it off, or Halvo did after it failed several times in a row. A switch-on by the person ends both, and a start by the person also ends the second. The interface run uses it where the capture run says paused. | interface | yes |
| Not yet | The status of an agent whose kind of task Halvo cannot do yet. | interface | no |
| Start now | The control that starts the task of an agent, or puts it in line to start when the agent before it is done. | interface | no |
| Remove | The control that allows an agent to remove the items that a confirmation shows. | interface | no |
| Keep | The control that refuses that removal. | interface | no |
| site | A website that an agent works on. A row and a sentence name it by a short name, such as Site1; a confirmation and a setup show its address as it is bound. | interface | no |
| its own window | The browser window of an agent, where the person signs in or does a step for the agent. | interface | no |
| Quick | The read-only label of an agent whose stored reasoning level is low. | interface | no |
| Standard | The read-only label of an agent whose stored reasoning level is medium. | interface | no |
| Thorough | The read-only label of an agent whose stored reasoning level is high. | interface | no |
