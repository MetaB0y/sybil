# Retained runs

Each publishable run gets an immutable directory named `<date>-v<protocol>`.
The directory must contain the copied protocol, raw JSONL, machine metadata,
validated summaries, and all generated figures. Do not replace or edit a run
in place; a changed protocol, implementation, machine, or rerun gets a new
directory.
