# Our commands, their toolbar

The case an embedder actually has: a product that already owns its chrome and
wants a spreadsheet inside it, not a spreadsheet application beside it.

Every button on this page is the host's own markup. Every one runs an OpenCalc
command by id, and the editor's own chrome is entirely off.

## The three things worth copying

**Ask what exists.** `listCommands()` returns every id this build has. A host
that hard-codes a list gets a button that silently does nothing the day an id
changes. Here an unknown id is *removed* rather than disabled — a control for a
capability the build does not have is not "temporarily unavailable", it is not
a thing.

**Turn chrome off by region, not with CSS.** `chrome({ toolbar: false, … })`
names regions because a host reaching into the shadow root breaks the moment
that markup moves. Hiding chrome with a stylesheet also leaves it *focusable*:
a menu your users can open with a keyboard and you cannot see.

**Check `source` before saving.** `cellsChanged` fires for the host's own
writes too, with `source: "api"`. A host that saves on every change saves its
own echo, forever.

## What this is not

It is not read-only — that is `../viewer`. A host toolbar and an access level
are different axes: this page has full edit rights and no OpenCalc chrome,
which is the combination the other samples do not show.
