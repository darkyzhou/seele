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
      rlimit: {
        fsize: {
          hard: 1024,
          soft: 1024,
        },
      },
    },
    fd: {
      stdout: "$TEMP_PATH/stdout.txt",
    },
  },
  check: (report) => {
    assert.strictEqual(report.status, "NORMAL");
  },
};
