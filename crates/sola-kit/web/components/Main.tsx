import { signal, computed } from "@preact/signals";

const count = signal(0);
const doubled = computed(() => count.value * 2);

export function Main() {
  return (
    <main>
      <h1>sola-kit</h1>
      <p>
        count: {count} · doubled: {doubled}
      </p>
      <button onClick={() => count.value++}>+1</button>
      <button onClick={() => (count.value = 0)}>reset</button>
    </main>
  );
}
