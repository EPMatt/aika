-- Native meeting intent travels with each durable step, including compensation.
ALTER TABLE outbox ADD COLUMN meeting_intent TEXT DEFAULT '{"beforeGuests":null,"guests":null,"notifications":"all"}';
PRAGMA user_version=8;
