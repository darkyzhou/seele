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
    },
  },
  check: (report) => {
    assert.strictEqual(report.status, "TIME_LIMIT_EXCEEDED");
  },
};