"use client";

// `"use client"` is not optional. The element touches `window` and `document`
// at import, and canvas cannot render on a server — so this file, and anything
// that imports it eagerly, has to stay off the server.
import dynamic from "next/dynamic";

// Loaded with `ssr: false` rather than merely marked client-side. A client
// component still gets *rendered* on the server for the initial HTML, which
// would run the import and fail there.
export const Sheet = dynamic(
  () => import("./SheetClient").then((m) => m.SheetClient),
  {
    ssr: false,
    loading: () => <div style={{ height: 600, background: "#f7f8fa", borderRadius: 12 }} />,
  },
);
