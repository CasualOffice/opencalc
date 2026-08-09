# Vanilla

The smallest integration that does something useful: mount, open a file, save
one back. No build step — open `index.html` through the dev server.

```bash
python3 ../../../webapp/serve.py     # then visit /../sdk/examples/vanilla/
```

Two things worth copying:

- **`sheet.ready`** resolves when the engine is up. Calling the API before that
  is fine — every method awaits it — but a host usually wants to know.
- **`e.source`** on a change event. A host that persists on change and loads on
  mount will echo its own writes back to itself forever without it.
