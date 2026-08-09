# View-only and preview

Two modes on one page, because the difference is easier to see than to explain.

|  | `view` | `preview` |
| --- | --- | --- |
| What it is | an access level | a presentation |
| Chrome | all of it, minus writing commands | none |
| Select and copy | yes | yes |
| Scroll, zoom, navigate sheets | yes | scroll only |
| Reads as | the application, without the editing | a picture of a document |

Both refuse writes **in the engine**. The chrome is how each is communicated,
not how it is enforced: a read-only mode that only hides buttons is read-only
right up until someone calls the API.

This page also shows three instances on one page sharing a single parsed
stylesheet — the thing that makes multiple embeds affordable.
