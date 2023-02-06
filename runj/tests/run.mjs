import { spawn } from "node:child_process";
import { chmod, mkdir, rm, readdir, stat } from "node:fs/promises";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

const RUNJ_PATH = resolve(fileURLToPath(import.meta.url), "../../bin/runj");
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
  config.rootless = rootless;

  const tempPath = resolve(
    tmpdir(),
    `runj-test-${Math.round(Math.random() * 100000)}`
  );
  await mkdir(tempPath);
  await chmod(tempPath, 0o777);

  const json = JSON.stringify(config)
    .replaceAll("$IMAGE_ROOTFS_PATH", IMAGE_ROOTFS_PATH)
    .replaceAll("$TEMP_PATH", tempPath);
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
  }).finally(() => {
    rm(tempPath, {
      recursive: true,
      force: true,
    });
  });
}
