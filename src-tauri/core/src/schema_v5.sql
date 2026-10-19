-- Incoming sync metadata is native-only and contains no credentials.
ALTER TABLE calendars ADD COLUMN remote_metadata TEXT;
ALTER TABLE accounts ADD COLUMN selection_confirmed INTEGER NOT NULL DEFAULT 0;
UPDATE accounts SET selection_confirmed=1 WHERE EXISTS(SELECT 1 FROM calendars WHERE account_id=accounts.id);
ALTER TABLE accounts ADD COLUMN sync_epoch INTEGER NOT NULL DEFAULT 0;
ALTER TABLE accounts ADD COLUMN sync_failure TEXT;
ALTER TABLE accounts ADD COLUMN last_sync_success TEXT;
CREATE TRIGGER invalidate_account_sync AFTER UPDATE OF connection_state ON accounts
BEGIN UPDATE accounts SET sync_epoch=sync_epoch+1 WHERE id=NEW.id; END;
CREATE TRIGGER invalidate_selection_sync AFTER UPDATE OF selected,active ON calendars
WHEN OLD.selected!=NEW.selected OR OLD.active!=NEW.active
BEGIN UPDATE accounts SET sync_epoch=sync_epoch+1 WHERE id=NEW.account_id; END;
CREATE TRIGGER invalidate_new_calendar_sync AFTER INSERT ON calendars WHEN NEW.account_id IS NOT NULL
BEGIN UPDATE accounts SET sync_epoch=sync_epoch+1 WHERE id=NEW.account_id; END;
CREATE TABLE incoming_items(id TEXT PRIMARY KEY REFERENCES remote_events(id) ON DELETE CASCADE, payload TEXT NOT NULL);
CREATE TABLE incoming_projections(id TEXT PRIMARY KEY REFERENCES remote_events(id) ON DELETE CASCADE,
    master_remote_id TEXT NOT NULL);
PRAGMA user_version=5;
