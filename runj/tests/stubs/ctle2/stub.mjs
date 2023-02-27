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
      time_ms: 1000,
    },
  },
  check: (report) => {
    assert.strictEqual(report.status, "USER_TIME_LIMIT_EXCEEDED");
  },
};
