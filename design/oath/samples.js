// Invented, so the comps can show states novus is not in today. Everything
// here wears a "sample" tag on the page. The real catalog is catalog.js.
window.SAMPLES = {
  // A local build whose commit is not on origin.
  unpushed: {
    'sola-browser': {
      h: 'b7c41d09e2aa',
      src: { repo: 'https://github.com/moonlight-pm/Sola', commit: '4f1e9a20', branch: 'tab-icons', dirty: true, by: 'home@novus', at: '2026-11-02T22:10:00Z' },
    },
  },
  // Packages outside Sola that the newest release moves. (Sola's own newest
  // pins are real, from CURRENT.md.)
  updates: {
    openssl: { newest: 'c18e7702ab4f', ver: '4.0.4' },
    curl: { newest: '9a03fd11c6e2', ver: '8.23.1' },
    git: { newest: 'e41bb0937d5a', ver: '2.56.1' },
  },
  // Oath's advisory file has id, url and summary. Severity, component and
  // fixed-in are what the comp wishes it had.
  advisories: {
    openssl: [
      { id: 'SAMPLE-0001', severity: 'High', component: 'openssl 4.0.3', fixed: '4.0.4', summary: 'Out-of-bounds read while parsing an X.509 name.', url: 'https://example.invalid/SAMPLE-0001' },
      { id: 'SAMPLE-0002', severity: 'Low', component: 'openssl 4.0.3', fixed: '4.0.4', summary: 'Timing side channel in a rarely used curve.', url: 'https://example.invalid/SAMPLE-0002' },
    ],
    curl: [
      { id: 'SAMPLE-0003', severity: 'Medium', component: 'curl 8.23.0', fixed: '8.23.1', summary: 'A cookie can follow a cross-origin redirect.', url: 'https://example.invalid/SAMPLE-0003' },
    ],
  },
  // On the origin, in no catalog on this machine.
  remote: [
    { n: 'ffmpeg', ver: '8.1', desc: 'Records, converts and streams audio and video.', home: 'https://ffmpeg.org/', needs: ['zlib', 'xz', 'alsa-lib'], fam: 'Machines', sz: 94000000 },
    { n: 'gimp', ver: '3.2.0', desc: 'Image editor.', home: 'https://www.gimp.org/', needs: ['cairo', 'pango', 'glib'], fam: 'Machines', sz: 310000000 },
  ],
};
