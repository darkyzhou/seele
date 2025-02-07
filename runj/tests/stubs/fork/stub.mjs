import { fileURLToPath } from "node:url";
import { resolve } from "node:path";
import assert from "node:assert";

export default {
  config: {
    cwd: "/",
    command: ["main"],
    mounts: [
      {
        from: `${resolve(fileURLToPath(import.meta.url), "../main")}`,
        to: "/usr/local/bin/main",
        options: ["exec"],
      },
    ],
    limits: {
      time_ms: 2000,
      cgroup: {
        pids_limit: 8,
      },
    },
  },
  check: (report) => {
    assert.strictEqual(report.status, "MEMORY_LIMIT_EXCEEDED");
  },
};
