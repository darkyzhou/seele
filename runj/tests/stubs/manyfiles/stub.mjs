import { fileURLToPath } from "node:url";
import { resolve } from "node:path";
import assert from "node:assert";

export default {
  config: {
    rootfs: "$IMAGE_ROOTFS_PATH",
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
      time: {
        wall: 1000,
      },
      rlimit: [
        {
          type: "RLIMIT_NOFILE",
          hard: 32,
          soft: 32,
        },
      ],
    },
  },
  check: (report) => {
    assert.strictEqual(report.status, "SIGNAL_TERMINATE");
    // The `fopen()` call returns `null` when exceeds the limit
    assert.strictEqual(report.exit_code, 139);
    assert.strictEqual(report.signal, "SIGSEGV");
  },
};
