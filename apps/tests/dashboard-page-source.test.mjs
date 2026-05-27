import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import test from "node:test";

const appsRoot = path.resolve(import.meta.dirname, "..");
const pagePath = path.join(appsRoot, "src", "app", "page.tsx");

test("管理员用量图表使用采样消耗文案且额度数值不加粗", async () => {
  const source = await fs.readFile(pagePath, "utf8");

  assert.match(source, /t\("5 小时额度采样消耗"\)/);
  assert.match(source, /t\("当日采样消耗"\)/);
  assert.match(source, /t\("基于用量快照正向变化估算"\)/);

  const quotaBranchStart = source.indexOf('if (dataKey === "totalConsumedPercent")');
  const quotaBranchEnd = source.indexOf("const row = item.payload", quotaBranchStart);
  assert.ok(quotaBranchStart >= 0 && quotaBranchEnd > quotaBranchStart);
  const quotaBranch = source.slice(quotaBranchStart, quotaBranchEnd);
  assert.equal(
    quotaBranch.includes("valueClassName"),
    false,
    "quota estimate tooltip value should keep normal weight",
  );
});

