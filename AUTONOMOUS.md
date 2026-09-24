# AUTONOMOUS.md — how to work unattended in this repo

- Work milestone by milestone from `AXIOM_SHADER_LAB.md` §4. Stop at every [STOP]: write the report under `lab/reports/` or the milestone doc, commit, and wait.
- Log every hypothesis and outcome (including negatives) in `lab/NOTEBOOK.md` before moving on.
- Prefer parallel sub-agents for independent shaders or independent components; never let two jobs share a device.
- Lock GPU clocks only for the duration of a timing batch; always reset with `nvidia-smi -rgc`.
- If a tool, device, or dependency is missing, record it in `lab/TOOLS.md` or `lab/DISCOVERY.md` and stop that branch of work rather than approximating.
- End each session with a handoff note in `lab/NOTEBOOK.md`: state, next step, open questions.
