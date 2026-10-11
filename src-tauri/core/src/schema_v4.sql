-- All migrations run in the repository's single immediate transaction.
CREATE TABLE accounts(id TEXT PRIMARY KEY, provider_id TEXT NOT NULL, remote_subject TEXT NOT NULL,
    display_name TEXT NOT NULL, connection_state TEXT NOT NULL, UNIQUE(provider_id,remote_subject), UNIQUE(id,provider_id));
ALTER TABLE calendars ADD COLUMN provider_id TEXT NOT NULL DEFAULT 'local';
ALTER TABLE calendars ADD COLUMN account_id TEXT REFERENCES accounts(id);
ALTER TABLE calendars ADD COLUMN remote_id TEXT;
ALTER TABLE calendars ADD COLUMN writable INTEGER NOT NULL DEFAULT 1 CHECK(writable IN (0,1));
ALTER TABLE calendars ADD COLUMN selected INTEGER NOT NULL DEFAULT 1 CHECK(selected IN (0,1));
ALTER TABLE calendars ADD COLUMN active INTEGER NOT NULL DEFAULT 1 CHECK(active IN (0,1));
ALTER TABLE calendars ADD COLUMN capabilities TEXT;
CREATE UNIQUE INDEX remote_calendar_identity ON calendars(account_id,remote_id);
CREATE TRIGGER calendar_account_insert BEFORE INSERT ON calendars WHEN
    (NEW.account_id IS NULL AND (NEW.provider_id!='local' OR NEW.remote_id IS NOT NULL)) OR
    (NEW.account_id IS NOT NULL AND (NEW.remote_id IS NULL OR NOT EXISTS(SELECT 1 FROM accounts WHERE id=NEW.account_id AND provider_id=NEW.provider_id)))
    BEGIN SELECT RAISE(ABORT,'calendar/account identity mismatch'); END;
CREATE TABLE remote_events(id TEXT PRIMARY KEY, account_id TEXT NOT NULL REFERENCES accounts(id),
    calendar_id TEXT NOT NULL REFERENCES calendars(id), remote_id TEXT NOT NULL, revision TEXT,
    shadow_payload TEXT NOT NULL, capabilities TEXT NOT NULL, UNIQUE(account_id,calendar_id,remote_id));
CREATE TRIGGER remote_event_scope BEFORE INSERT ON remote_events WHEN NOT EXISTS(
    SELECT 1 FROM calendars WHERE id=NEW.calendar_id AND account_id=NEW.account_id)
    BEGIN SELECT RAISE(ABORT,'event/account identity mismatch'); END;
CREATE TABLE outbox(id TEXT PRIMARY KEY, account_id TEXT NOT NULL REFERENCES accounts(id),
    calendar_id TEXT NOT NULL REFERENCES calendars(id), event_id TEXT NOT NULL,
    operation TEXT NOT NULL CHECK(operation IN ('create','update','delete')), before_payload TEXT, after_payload TEXT,
    expected_revision TEXT, state TEXT NOT NULL DEFAULT 'pending' CHECK(state IN ('pending','inFlight','acknowledged','blocked')),
    depends_on TEXT REFERENCES outbox(id), attempts INTEGER NOT NULL DEFAULT 0, retry_at TEXT, acknowledgement TEXT);
CREATE INDEX outbox_account_state ON outbox(account_id,state);
CREATE TABLE conflicts(id TEXT PRIMARY KEY, account_id TEXT NOT NULL REFERENCES accounts(id),
    calendar_id TEXT NOT NULL REFERENCES calendars(id), event_id TEXT NOT NULL, local_payload TEXT, remote_payload TEXT, remote_revision TEXT);
CREATE TABLE sync_cursors(calendar_id TEXT PRIMARY KEY REFERENCES calendars(id), cursor TEXT, last_success TEXT);
CREATE TABLE sync_generations(id TEXT PRIMARY KEY, calendar_id TEXT NOT NULL REFERENCES calendars(id), payload TEXT NOT NULL, final_cursor TEXT, complete INTEGER NOT NULL DEFAULT 0);
CREATE TABLE coverage(calendar_id TEXT NOT NULL REFERENCES calendars(id), start TEXT NOT NULL, end TEXT NOT NULL, generation_id TEXT, PRIMARY KEY(calendar_id,start,end));
CREATE TABLE account_leases(account_id TEXT PRIMARY KEY REFERENCES accounts(id), owner TEXT NOT NULL, expires_at TEXT NOT NULL);
PRAGMA user_version=4;
