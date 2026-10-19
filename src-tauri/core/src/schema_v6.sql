-- Singleton backfill preserves every operation, dependency, attempt and acknowledgement.
CREATE TABLE operation_groups(
    id TEXT PRIMARY KEY, account_id TEXT NOT NULL REFERENCES accounts(id),
    calendar_id TEXT NOT NULL REFERENCES calendars(id));
CREATE TRIGGER operation_group_identity BEFORE INSERT ON operation_groups WHEN NOT EXISTS(
    SELECT 1 FROM calendars WHERE id=NEW.calendar_id AND account_id=NEW.account_id)
BEGIN SELECT RAISE(ABORT,'group/account identity mismatch'); END;
CREATE TABLE operation_group_series(
    group_id TEXT NOT NULL REFERENCES operation_groups(id) ON DELETE CASCADE,
    event_id TEXT NOT NULL, before_payload TEXT, after_payload TEXT,
    PRIMARY KEY(group_id,event_id));
CREATE INDEX group_series_event ON operation_group_series(event_id,group_id);
ALTER TABLE outbox ADD COLUMN group_id TEXT REFERENCES operation_groups(id);
ALTER TABLE outbox ADD COLUMN step_index INTEGER NOT NULL DEFAULT 0 CHECK(step_index>=0);
ALTER TABLE outbox ADD COLUMN resource_id TEXT;
ALTER TABLE outbox ADD COLUMN series_remote_id TEXT;
ALTER TABLE outbox ADD COLUMN original_start TEXT;
INSERT INTO operation_groups SELECT id,account_id,calendar_id FROM outbox ORDER BY rowid;
INSERT INTO operation_group_series SELECT id,event_id,before_payload,after_payload FROM outbox;
UPDATE outbox SET group_id=id,resource_id=(SELECT remote_id FROM remote_events WHERE id=outbox.event_id)
    WHERE operation!='create' OR attempts>0;
UPDATE outbox SET group_id=id WHERE group_id IS NULL;
-- Reserve create identities for the whole action before its first HTTP request.
UPDATE outbox SET resource_id=lower(hex(randomblob(16))) WHERE operation='create' AND attempts=0;
UPDATE outbox SET resource_id=coalesce((SELECT p.resource_id FROM outbox p
    WHERE p.event_id=outbox.event_id AND p.operation='create' AND p.state!='acknowledged'
    AND p.rowid<outbox.rowid ORDER BY p.rowid DESC LIMIT 1),resource_id) WHERE operation!='create';
UPDATE outbox SET resource_id=(SELECT p.resource_id FROM outbox p WHERE p.event_id=outbox.event_id
    AND p.rowid<outbox.rowid AND p.resource_id IS NOT NULL ORDER BY p.rowid DESC LIMIT 1)
    WHERE resource_id IS NULL;
CREATE UNIQUE INDEX outbox_group_order ON outbox(group_id,step_index);
-- Existing callers (and recovery callers) automatically create singleton groups.
CREATE TRIGGER outbox_group_scope BEFORE INSERT ON outbox WHEN NEW.group_id IS NOT NULL
    AND NOT EXISTS(SELECT 1 FROM operation_groups WHERE id=NEW.group_id
        AND account_id=NEW.account_id AND calendar_id=NEW.calendar_id)
BEGIN SELECT RAISE(ABORT,'operation group scope mismatch'); END;
CREATE TRIGGER outbox_group_scope_update BEFORE UPDATE OF group_id,account_id,calendar_id ON outbox
    WHEN NEW.group_id IS NULL OR NOT EXISTS(SELECT 1 FROM operation_groups WHERE id=NEW.group_id
        AND account_id=NEW.account_id AND calendar_id=NEW.calendar_id)
BEGIN SELECT RAISE(ABORT,'operation group scope mismatch'); END;
CREATE TRIGGER outbox_group_member AFTER INSERT ON outbox
BEGIN
    INSERT OR IGNORE INTO operation_groups VALUES(coalesce(NEW.group_id,NEW.id),NEW.account_id,NEW.calendar_id);
    UPDATE outbox SET group_id=coalesce(NEW.group_id,NEW.id),resource_id=coalesce(NEW.resource_id,
        CASE WHEN NEW.operation='create' THEN lower(hex(randomblob(16))) ELSE coalesce(
            (SELECT resource_id FROM outbox p WHERE p.event_id=NEW.event_id AND p.operation='create'
                AND p.state!='acknowledged' AND p.rowid<NEW.rowid ORDER BY p.rowid DESC LIMIT 1),
            (SELECT remote_id FROM remote_events WHERE id=NEW.event_id)) END)
        WHERE id=NEW.id;
    INSERT INTO operation_group_series(group_id,event_id,before_payload,after_payload)
    VALUES(coalesce(NEW.group_id,NEW.id),NEW.event_id,NEW.before_payload,NEW.after_payload)
    ON CONFLICT(group_id,event_id) DO UPDATE SET after_payload=excluded.after_payload;
END;
-- Includes acknowledged steps while ANY sibling is unfinished. Incoming publication,
-- conflict review and counts must use this lifetime rather than individual step state.
CREATE VIEW active_group_series AS
    SELECT s.*,g.account_id,g.calendar_id FROM operation_group_series s
    JOIN operation_groups g ON g.id=s.group_id
    WHERE EXISTS(SELECT 1 FROM outbox o WHERE o.group_id=s.group_id AND o.state!='acknowledged');
PRAGMA user_version=6;
