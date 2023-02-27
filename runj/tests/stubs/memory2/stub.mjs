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
      cgroup: {
        memory: 32 * 1024 * 1024,
      },
    },
  },
  check: (report) => {
    assert.strictEqual(report.status, "MEMORY_LIMIT_EXCEEDED");
    assert.strictEqual(report.memory_usage_kib < 12 * 1024, true);
  },
};
