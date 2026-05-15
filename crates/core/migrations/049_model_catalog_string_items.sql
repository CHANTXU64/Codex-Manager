CREATE TABLE IF NOT EXISTS model_catalog_string_items (
  scope TEXT NOT NULL,
  slug TEXT NOT NULL,
  item_kind TEXT NOT NULL,
  value TEXT NOT NULL,
  sort_index INTEGER NOT NULL DEFAULT 0,
  updated_at INTEGER NOT NULL,
  PRIMARY KEY (scope, slug, item_kind, value)
);

CREATE INDEX IF NOT EXISTS idx_model_catalog_string_items_scope_kind_sort
  ON model_catalog_string_items(scope, item_kind, slug, sort_index, value);

CREATE TABLE IF NOT EXISTS model_catalog_additional_speed_tiers (
  scope TEXT NOT NULL,
  slug TEXT NOT NULL,
  value TEXT NOT NULL,
  sort_index INTEGER NOT NULL DEFAULT 0,
  updated_at INTEGER NOT NULL,
  PRIMARY KEY (scope, slug, value)
);

CREATE TABLE IF NOT EXISTS model_catalog_experimental_supported_tools (
  scope TEXT NOT NULL,
  slug TEXT NOT NULL,
  value TEXT NOT NULL,
  sort_index INTEGER NOT NULL DEFAULT 0,
  updated_at INTEGER NOT NULL,
  PRIMARY KEY (scope, slug, value)
);

CREATE TABLE IF NOT EXISTS model_catalog_input_modalities (
  scope TEXT NOT NULL,
  slug TEXT NOT NULL,
  value TEXT NOT NULL,
  sort_index INTEGER NOT NULL DEFAULT 0,
  updated_at INTEGER NOT NULL,
  PRIMARY KEY (scope, slug, value)
);

CREATE TABLE IF NOT EXISTS model_catalog_available_in_plans (
  scope TEXT NOT NULL,
  slug TEXT NOT NULL,
  value TEXT NOT NULL,
  sort_index INTEGER NOT NULL DEFAULT 0,
  updated_at INTEGER NOT NULL,
  PRIMARY KEY (scope, slug, value)
);

INSERT OR REPLACE INTO model_catalog_string_items (scope, slug, item_kind, value, sort_index, updated_at)
SELECT scope, slug, 'additional_speed_tiers', value, sort_index, updated_at
FROM model_catalog_additional_speed_tiers;

INSERT OR REPLACE INTO model_catalog_string_items (scope, slug, item_kind, value, sort_index, updated_at)
SELECT scope, slug, 'experimental_supported_tools', value, sort_index, updated_at
FROM model_catalog_experimental_supported_tools;

INSERT OR REPLACE INTO model_catalog_string_items (scope, slug, item_kind, value, sort_index, updated_at)
SELECT scope, slug, 'input_modalities', value, sort_index, updated_at
FROM model_catalog_input_modalities;

INSERT OR REPLACE INTO model_catalog_string_items (scope, slug, item_kind, value, sort_index, updated_at)
SELECT scope, slug, 'available_in_plans', value, sort_index, updated_at
FROM model_catalog_available_in_plans;

DROP TABLE IF EXISTS model_catalog_additional_speed_tiers;
DROP TABLE IF EXISTS model_catalog_experimental_supported_tools;
DROP TABLE IF EXISTS model_catalog_input_modalities;
DROP TABLE IF EXISTS model_catalog_available_in_plans;
