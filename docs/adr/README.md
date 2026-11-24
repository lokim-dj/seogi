# Decisions

One file per decision that the next person, or agent, would otherwise undo.

Write one only when all three are true:

1. It is costly to change later.
2. Someone reading the code would wonder why it is this way.
3. There was a real alternative, and it was turned down for a reason.

## Shape

`docs/adr/0001-short-name.md`, numbered in order, never renumbered.

    # Short title

    Decided by: maintainer | agent (reason that can be checked again) · YYYY-MM-DD

    One to three sentences: the situation, what was decided, why.

    Turned down: the alternative, and why. (Only when worth remembering.)

Reversing a decision is the maintainer's call (AGENTS.md, Who decides). A decision that no longer holds is rewritten or deleted in the same pull request that changes the behaviour. Git keeps the old one.
