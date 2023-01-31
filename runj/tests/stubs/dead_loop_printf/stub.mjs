import { fileURLToPath } from "node:url";
import { resolve } from "node:path";
import assert from "node:assert";

export default {
  config: {
    rootfs: "$IMAGE_ROOTFS_PATH",
    cwd: '/',
    command: ["main"],
    mounts: [
      {
        from: `${resolve(fileURLToPath(import.meta.url), "../main")}`,
        to: "/usr/local/bin/main",
        options: ["exec"],
      },
    ],
    limits: {
      time: {
        wall: 300,
      },
      rlimit: [
        {
          type: "RLIMIT_FSIZE",
          hard: 1024,
          soft: 1024,
        },
      ],
    },
    fd: {
      stdout: "$TEMP_PATH/stdout.txt",
    },
  },
  check: (report) => {
    assert.strictEqual(report.status, "OUTPUT_LIMIT_EXCEEDED");
  },
};
