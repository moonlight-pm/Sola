import { h, render } from 'preact';
import { signal } from '@preact/signals';

const count = signal(0);

function Hello() {
  return (
    <main style="padding: 2rem; font-family: system-ui">
      <h1>sola-kit · preact bootstrap</h1>
      <p>signal counter: {count}</p>
      <button onClick={() => count.value++}>+1</button>
    </main>
  );
}

const target = document.getElementById('app');
if (target) render(<Hello />, target);
