import { fileURLToPath } from "node:url";
import { resolve } from "node:path";
import assert from "node:assert";

export default {
  config: {
    cwd: "/seele",
    command: ["gcc", "main.c"],
    mounts: [
      {
        from: "$TEMP_PATH",
        to: "/seele",
        options: ["rw"],
      },
      {
        from: `${resolve(fileURLToPath(import.meta.url), "../main.c")}`,
        to: "/seele/main.c",
      },
    ],
    limits: {
      time_ms: 1000,
      cgroup: {
        memory: 32 * 1024 * 1024,
      },
    },
  },
  check: (report) => {
    assert.strictEqual(report.status, "USER_TIME_LIMIT_EXCEEDED");
  },
};
