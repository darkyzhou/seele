import { fileURLToPath } from "node:url";
import { resolve } from "node:path";
import assert from "node:assert";

export default {
  config: {
    rootfs: "$IMAGE_ROOTFS_PATH",
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
      cgroup: {
        memory: 32 * 1024 * 1024,
        memory_swap: 32 * 1024 * 1024,
      },
    },
  },
  check: (report) => {
    assert.strictEqual(report.status, "MEMORY_LIMIT_EXCEEDED");
  },
};
