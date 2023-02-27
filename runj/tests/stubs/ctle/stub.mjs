import { fileURLToPath } from "node:url";
import { resolve } from "node:path";
import assert from "node:assert";

export default {
  config: {
    cwd: "/seele",
    command: ["g++", "main.cpp"],
    mounts: [
      {
        from: "$TEMP_PATH",
        to: "/seele",
        options: ["rw"],
      },
      {
        from: `${resolve(fileURLToPath(import.meta.url), "../main.cpp")}`,
        to: "/seele/main.cpp",
      },
    ],
    limits: {
      time_ms: 10000,
      cgroup: {
        memory: 32 * 1024 * 1024, // 32 MiB
      },
    },
  },
  check: (report) => {
    assert.strictEqual(report.status, "MEMORY_LIMIT_EXCEEDED");
  },
};
