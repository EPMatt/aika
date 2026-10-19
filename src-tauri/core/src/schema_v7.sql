-- Cleanup records retain the account until credentials have been deleted successfully.
ALTER TABLE accounts ADD COLUMN removal_in_progress INTEGER NOT NULL DEFAULT 0;
CREATE TABLE identity_lifecycles(identity_hash TEXT PRIMARY KEY, epoch INTEGER NOT NULL, state TEXT NOT NULL);
CREATE TABLE lifecycle_clock(id INTEGER PRIMARY KEY CHECK(id=1), epoch INTEGER NOT NULL);
INSERT INTO lifecycle_clock VALUES(1,0);
CREATE TRIGGER fence_removing_account BEFORE UPDATE OF connection_state ON accounts
WHEN OLD.removal_in_progress=1 AND NEW.connection_state!='"disconnected"'
BEGIN SELECT RAISE(ABORT,'account removal in progress'); END;
PRAGMA user_version=7;
