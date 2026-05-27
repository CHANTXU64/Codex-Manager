import assert from "node:assert/strict";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { pathToFileURL } from "node:url";
import ts from "../node_modules/typescript/lib/typescript.js";

const appsRoot = path.resolve(import.meta.dirname, "..");
const sourcePath = path.join(
  appsRoot,
  "src",
  "lib",
  "api",
  "dashboard-client.ts",
);

async function loadDashboardClientModule() {
  const source = await fs.readFile(sourcePath, "utf8");
  const compiled = ts.transpileModule(source, {
    compilerOptions: {
      module: ts.ModuleKind.ES2022,
      target: ts.ScriptTarget.ES2022,
    },
    fileName: sourcePath,
  });

  const tempDir = await fs.mkdtemp(
    path.join(os.tmpdir(), "codexmanager-dashboard-client-"),
  );
  const tempFile = path.join(tempDir, "dashboard-client.mjs");
  await fs.writeFile(
    tempFile,
    compiled.outputText
      .replace('from "./transport"', 'from "./transport.mjs"')
      .replace('from "./normalize"', 'from "./normalize.mjs"'),
    "utf8",
  );
  await fs.writeFile(
    path.join(tempDir, "transport.mjs"),
    "export async function invoke() { return {}; }\nexport function withAddr(value) { return value; }\n",
    "utf8",
  );
  await fs.writeFile(
    path.join(tempDir, "normalize.mjs"),
    "export function normalizeModelCatalog(value) { return value; }\nexport function normalizeRequestLogs(value) { return value; }\n",
    "utf8",
  );
  return import(pathToFileURL(tempFile).href);
}

const dashboardClient = await loadDashboardClientModule();

test("normalizeDashboardAdminUsageSummary 保留缺失额度日为 null", () => {
  const summary = dashboardClient.normalizeDashboardAdminUsageSummary({
    range_start_ts: 100,
    range_end_ts: 200,
    today_start_ts: 100,
    today_end_ts: 200,
    daily_usage: [],
    daily_quota_consumption: [
      {
        day_start_ts: 100,
        day_end_ts: 200,
        total_consumed_percent: null,
      },
    ],
  });

  assert.equal(summary.dailyQuotaConsumption.length, 1);
  assert.equal(summary.dailyQuotaConsumption[0].totalConsumedPercent, null);
});

