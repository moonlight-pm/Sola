import { defineConfig } from "cf/config";

// The public name is not in the repo: the workflow passes it in. Without one
// the site deploys to its workers.dev address only.
const domain = process.env.SITE_DOMAIN;

export default defineConfig({
  worker: {
    name: "sola-site",
    compatibilityDate: "2026-10-23",
    assets: {
      htmlHandling: "auto-trailing-slash",
      notFoundHandling: "404-page",
    },
    domains: domain ? [domain] : [],
    workersDev: !domain,
  },
});
