CREATE TABLE IF NOT EXISTS kv_data (
	scope      TEXT NOT NULL,
	namespace  TEXT NOT NULL,
	key        TEXT NOT NULL,
	value      BYTEA NOT NULL,
	updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
	PRIMARY KEY (scope, namespace, key)
);

CREATE TABLE IF NOT EXISTS stats_conversations (
	scene TEXT NOT NULL,
	id TEXT NOT NULL,
	name TEXT NOT NULL DEFAULT '',
	msg_count BIGINT NOT NULL DEFAULT 0,
	friend BOOLEAN NOT NULL DEFAULT false,
	PRIMARY KEY (scene, id)
);

ALTER TABLE stats_conversations ADD COLUMN IF NOT EXISTS friend BOOLEAN NOT NULL DEFAULT false;

CREATE TABLE IF NOT EXISTS stats_daily (
	day TEXT PRIMARY KEY,
	recv BIGINT NOT NULL DEFAULT 0,
	sent BIGINT NOT NULL DEFAULT 0,
	button BIGINT NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS stats_migrations (
	name TEXT PRIMARY KEY,
	at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS stats_conversations_msg ON stats_conversations(msg_count DESC);

CREATE TABLE IF NOT EXISTS inbox_conversations (
	id           TEXT PRIMARY KEY,
	kind         TEXT NOT NULL,
	name         TEXT NOT NULL DEFAULT '',
	last_at      BIGINT NOT NULL DEFAULT 0,
	last_preview TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS inbox_messages (
	id              BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
	conv_id         TEXT NOT NULL,
	direction       TEXT NOT NULL,
	sender_id       TEXT NOT NULL DEFAULT '',
	sender_name     TEXT NOT NULL DEFAULT '',
	content         TEXT NOT NULL DEFAULT '',
	platform_msg_id TEXT NOT NULL DEFAULT '',
	created_at      BIGINT NOT NULL
);

CREATE INDEX IF NOT EXISTS inbox_messages_conv ON inbox_messages(conv_id, id);
CREATE INDEX IF NOT EXISTS inbox_messages_at ON inbox_messages(created_at);
