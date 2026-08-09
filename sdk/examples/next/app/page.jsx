import { Sheet } from "./Sheet";

export default function Page() {
  return (
    <main style={{ padding: 24, fontFamily: "system-ui, sans-serif" }}>
      <h1 style={{ fontSize: 20 }}>OpenCalc in Next.js</h1>
      <Sheet />
    </main>
  );
}
