import { fileURLToPath } from "node:url";
import { resolve } from "node:path";
import assert from "node:assert";

export default {
  config: {
    rootfs: "$IMAGE_ROOTFS_PATH",
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
      time: {
        wall: 10000,
      },
      rlimit: [
        {
          type: "RLIMIT_FSIZE",
          hard: 102400,
          soft: 102400,
        },
      ],
    },
  },
  check: (report) => {
    // Gcc will handle SIGXFSZ and exits with code 4
    assert.strictEqual(report.status, "RUNTIME_ERROR");
    assert.strictEqual(report.exit_code, 4);
  },
};
