import { merge } from "lodash-es";
import { spawn } from "node:child_process";
import { chmod, mkdir, rm, readdir, stat } from "node:fs/promises";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

const RUNJ_PATH = resolve(fileURLToPath(import.meta.url), "../../bin/runj");
const TEMP_PATH = resolve(tmpdir(), "runj-test");
const IMAGE_ROOTFS_PATH = resolve(
  fileURLToPath(import.meta.url),
  "../image/rootfs"
);

try {
  await stat(RUNJ_PATH);
  await stat(IMAGE_ROOTFS_PATH);
} catch (e) {
  throw new Error(`Missing required files: ${e.message}`);
}

for (const name of (await readdir("./stubs")).sort()) {
  console.log(`>>> Testing ${name}`);

  const path = resolve("stubs", name);
  const stubPath = resolve(path, "stub.mjs");

  const stub = (await import(stubPath)).default;
  try {
    await runTest(stub, true);
    console.log("OK\n");
  } catch (e) {
    console.error(`ERR: ${e.message ?? e}\n`);
  }
}

await rm(TEMP_PATH, {
  recursive: true,
  force: true,
});

async function runTest(stub, rootless = true) {
  const report = await executeRunj(stub.config, rootless);
  try {
    stub.check(report);
  } catch (e) {
    console.error(JSON.stringify(report, null, 2));
    throw new Error(`Assertion failed: ${e.message}`);
  }
}

async function executeRunj(config, rootless = true) {
  const mergedConfig = merge(
    {
      overlayfs: {
        lower_dir: IMAGE_ROOTFS_PATH,
        upper_dir: await makeTempPath("upperdir"),
        work_dir: await makeTempPath("workdir"),
        merged_dir: await makeTempPath("merged"),
      },
      // FIXME: adapt to ci/cd
      user_namespace: {
        enabled: true,
        root_uid: 1000,
        uid_map_begin: 100000,
        uid_map_count: 65536,
        root_gid: 1000,
        gid_map_begin: 100000,
        gid_map_count: 65536,
      },
      limits: {
        time_ms: 3000,
        cgroup: {
          memory: 128 * 1024 * 1024, // 128 MiB
          pids_limit: 16,
        },
        rlimit: {
          fsize: {
            hard: 1 * 1024 * 1024, // 1 MiB
            soft: 1 * 1024 * 1024,
          },
          core: {
            hard: 0,
            soft: 0,
          },
          no_file: {
            hard: 64,
            soft: 64,
          },
        },
      },
    },
    config
  );

  const json = JSON.stringify(mergedConfig)
    .replaceAll("$IMAGE_ROOTFS_PATH", IMAGE_ROOTFS_PATH)
    .replaceAll("$TEMP_PATH", await makeTempPath("mount"));

  return new Promise((resolve, reject) => {
    try {
      const runj = spawn(RUNJ_PATH, {
        timeout: 60000,
        killSignal: "SIGKILL",
      });

      runj.stdin.write(json);
      runj.stdin.end();

      let output = "";
      runj.stdout.on("data", (data) => {
        output += data.toString();
      });

      let stderrOutput = "";
      runj.stderr.on("data", (data) => {
        stderrOutput += data.toString();
      });

      runj.on("close", (code) => {
        if (code) {
          reject(
            new Error(
              `Runj failed with code ${code}: ${output} ${stderrOutput}`
            )
          );
        } else {
          let data;
          try {
            data = JSON.parse(output);
          } catch (e) {
            reject(new Error(`Error parsing output: ${output}`));
            return;
          }

          resolve(data);
        }
      });
    } catch (err) {
      reject(err);
    }
  });
}

async function makeTempPath(prefix) {
  const tempPath = resolve(
    TEMP_PATH,
    prefix,
    `${Math.round(Math.random() * 100000)}`
  );
  await mkdir(tempPath, { recursive: true });
  await chmod(tempPath, 0o777);
  return tempPath;
}
