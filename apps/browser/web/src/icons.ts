// Lucide icon SVGs (24x24 viewBox, rendered at 14px)
// Source: https://lucide.dev

function icon(path: string): string {
  return `<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">${path}</svg>`;
}

export const ArrowLeft = icon('<path d="m12 19-7-7 7-7"/><path d="M19 12H5"/>');
export const ArrowRight = icon('<path d="m12 5 7 7-7 7"/><path d="M5 12h14"/>');
export const RotateCw = icon('<path d="M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8"/><path d="M21 3v5h-5"/>');
export const Plus = icon('<path d="M5 12h14"/><path d="M12 5v14"/>');
export const X = icon('<path d="M18 6 6 18"/><path d="m6 6 12 12"/>');
