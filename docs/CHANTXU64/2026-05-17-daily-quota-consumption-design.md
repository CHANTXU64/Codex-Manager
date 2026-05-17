# 每日额度消耗百分比统计

## 目标

在首页用量分析区域，新增「每日额度消耗百分比」图表，基于账号池5小时额度的快照数据，统计每日累计消耗百分比，并列于现有每日Token趋势图旁边。

## 背景

现有首页用量分析只展示 Token 维度的统计（每日Token趋势、用户/账号排名等）。账号池中的每个账号有5小时额度百分比（`usedPercent`），但首页没有基于此的消耗统计。

用户需要看到：每天账号池的额度被消耗了多少百分比。因为5小时额度会重置，一天内可能多次消耗，需要把所有消耗段累加。

## 核心算法：累计额度消耗计算

对同一账号的相邻快照（按 `captured_at` 排序）：

- **无重置**（`used_percent` 单调上升）：消耗 = `used_percent(N+1) - used_percent(N)`
- **有重置**（当前实现检测信号：`used_percent` 显著下降超过 10 个百分点）：消耗 = `0`。重置刷新本身不代表账号在重置边界前把剩余额度用完，不能把 `100 - used_percent(N)` 当作实际消耗。

将同一天内所有账号的所有正向消耗段累加，得到每日总消耗百分比。

重置检测信号：

1. `used_percent` 下降超过 10 个百分点（当前实现使用该辅助判断，避免快照精度误差导致的误判）
2. `resets_at` 可作为未来增强信号，但不能用于补记重置前剩余额度

### 示例

- 00:00 A账号已用55%（还剩45%），B账号已用0%（还剩100%）
- A继续使用到已用90%（消耗35%）
- 中午12点A重置到已用0%，该刷新本身不记消耗
- A重置后继续用到70%（消耗70%）
- B用了50%
- 当日总消耗 = 35 + 70 + 50 = 155%

## 数据结构

### 后端新增

```rust
struct DailyQuotaConsumption {
    day_start_ts: i64,
    total_consumed_percent: f64,
    by_account: Vec<AccountQuotaConsumption>,
}

struct AccountQuotaConsumption {
    account_id: String,
    account_label: String,
    consumed_percent: f64,
}
```

### API 变更

在 `DashboardAdminUsageSummary` 返回值中新增字段：

```typescript
interface DashboardAdminUsageSummary {
  // ...existing fields...
  dailyQuotaConsumption: DailyQuotaConsumption[];
}
```

## 后端实现

1. **在快照写入路径累计每日额度消耗**：在 `crates/service/src/usage/usage_snapshot_store.rs` 中
   - 写入新快照前读取同账号上一条快照
   - 跳过长窗口、免费账号、缺失 `used_percent` 的快照
   - 对相邻快照计算正向增量；检测到 5h 重置时不补记上周期剩余额度
   - 将增量写入 `quota_consumption_daily`，按本地日聚合

2. **集成到 `read_admin_usage_summary`**：在 `crates/service/src/dashboard.rs` 中读取 `quota_consumption_daily`，填充 `dailyQuotaConsumption` 返回值

3. **填充空白天**：与现有 `fill_daily_usage` 类似，确保7天范围内没有数据的天显示为0

4. **轮询频率**：默认账号用量轮询间隔为 120 秒；保留 `CODEXMANAGER_USAGE_POLL_INTERVAL_SECS` 和设置页覆盖能力

## 前端实现

1. **扩展类型定义**：在 `DashboardAdminUsageSummary` 中新增 `dailyQuotaConsumption` 字段

2. **新增 `DailyQuotaConsumptionChart` 组件**：
   - 使用 Recharts BarChart
   - 如果有按账号拆分数据，使用堆叠柱状图（Stacked BarChart），每个账号一个色段
   - 如果无拆分数据或账号过多，使用普通柱状图显示总量
   - Y轴标签："消耗百分比 (%)"
   - X轴：日期
   - 展示7天趋势

3. **布局调整**：在 `page.tsx` 的 admin dashboard 区域，将新图表放置在现有 `DailyTokenLineChart` 旁边（并列），使用 grid 布局

4. **Tooltip**：悬停显示当天总消耗百分比，以及各账号的明细消耗

## 限制与边界情况

- **快照频率不足**：如果账号在两次快照之间使用并重置，仍可能漏掉部分消耗段。默认轮询已调至 120 秒以降低漏记概率，但不能保证捕获所有边界
- **未知额度账号**：`used_percent` 为 null 的账号不参与计算
- **7天窗口账号**：通过 `window_minutes` / secondary 字段过滤，只统计5小时窗口的账号
- **免费账号**：通过 `isFreePlanUsage` 过滤，不参与统计
- **重置刷新**：`used_percent` 显著下降时视为进入新5小时周期，刷新本身不记消耗；例如 `87% -> 0%` 不会把剩余 `13%` 加到首页消耗统计
