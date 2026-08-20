-- 005_biometrics: templates that can travel between terminals, and that stay
-- unreadable in a copied database file.
--
-- Two gaps this closes.
--
-- The first is that fingerprint templates only ever arrived over the ADMS push
-- channel. A terminal reached directly on port 4370 — which is the only way in
-- once its cloud server is switched off — handed over its user table and
-- nothing else, so `member_fingerprints` stayed empty on exactly the machines
-- where a direct connection was the sole option. There was also no way to send
-- templates back. That is what provisioning a replacement terminal means, and
-- without it every member of staff re-enrols every finger by hand at the
-- keypad, which for this school is an afternoon of queueing.
--
-- The second is that templates were stored as plain base64. A fingerprint
-- template is not a password: staff cannot be issued a new finger after a leak.
-- This database is also deliberately easy to copy — the backup routine exists
-- to put it on a memory stick, and the README tells the office to keep those
-- copies off the machine. So the blob is now sealed with a key held outside the
-- database, and a copied `attendance.db` on its own yields nothing.
--
-- The plaintext `template` column is kept rather than dropped. Rows written by
-- an older build still have data in it, and the key does not exist inside SQL,
-- so the move to sealed storage cannot happen here. It happens in Rust on the
-- first run that has the key (`biosync::seal_legacy_templates`); this migration
-- only makes room for it. `enc_version` says which form a row is actually in:
-- 0 = legacy plaintext in `template`, 1 = sealed in `template_enc`.

-- ---------------------------------------------------------------------------
-- 1. Sealed template storage
-- ---------------------------------------------------------------------------

-- The sealed envelope: [version:1][nonce:24][ciphertext][tag:32].
ALTER TABLE member_fingerprints ADD COLUMN template_enc BLOB;

-- Keyed HMAC of the *plaintext* template, hex. Lets a re-sync tell "this finger
-- is unchanged" from "this finger was re-enrolled" without unsealing anything,
-- and is useless to anyone who does not hold the key — an unkeyed hash would
-- let someone confirm a guessed template, which is the whole risk with
-- biometric data.
ALTER TABLE member_fingerprints ADD COLUMN template_mac TEXT NOT NULL DEFAULT '';

-- 0 = plaintext base64 still in `template`, 1 = sealed in `template_enc`.
ALTER TABLE member_fingerprints ADD COLUMN enc_version INTEGER NOT NULL DEFAULT 0;

-- ZK finger algorithm the template was captured with (9, 10 or 12 in the
-- field). A template is only meaningful to a terminal running the same
-- algorithm, so uploading a v10 template to a v12 sensor produces a user who
-- silently cannot scan. Recorded so that mismatch can be reported rather than
-- discovered by a member of staff standing at the gate.
ALTER TABLE member_fingerprints ADD COLUMN algo_version INTEGER NOT NULL DEFAULT 0;

-- The device-internal row id the template was read against. Templates are keyed
-- on this on the wire, not on the enrolment number, so it has to survive the
-- round trip if they are ever to be written back.
ALTER TABLE member_fingerprints ADD COLUMN device_uid INTEGER;

-- Finding the rows still in the old form must not scan the table on every
-- start-up once the school has a few hundred fingers stored.
CREATE INDEX idx_fp_enc_version ON member_fingerprints(enc_version);

-- ---------------------------------------------------------------------------
-- 2. Device-side user fields
-- ---------------------------------------------------------------------------

-- `members` is already the local half of the device's user table: enroll_no,
-- device_name, privilege, card_no and device_password all mirror it. Two
-- fields were missing.

-- The device's internal row id for this user. Distinct from enroll_no: the
-- enrolment number is what staff type on the keypad, the uid is the slot the
-- firmware keeps them in, and the template table joins on the uid. Nullable
-- because a member created in the office has no slot on the terminal until they
-- are uploaded to it.
ALTER TABLE members ADD COLUMN device_uid INTEGER;

-- Whether the terminal should accept this person at all. The office needs a way
-- to stop a departed member of staff scanning without deleting their history,
-- and 'status' is an HR field the device knows nothing about.
ALTER TABLE members ADD COLUMN is_enabled INTEGER NOT NULL DEFAULT 1;

-- ---------------------------------------------------------------------------
-- 3. Sync history
-- ---------------------------------------------------------------------------

-- `sync_log` records one line per job. That is right for the dashboard and
-- useless when a transfer half-worked, which is the case that costs a day:
-- 118 of 120 users written, and nothing saying which two or why. This keeps the
-- counts a partial transfer produces so the failure is legible afterwards.
CREATE TABLE bio_sync_runs (
    id             INTEGER PRIMARY KEY,
    started_at     TEXT NOT NULL DEFAULT (datetime('now','localtime')),
    finished_at    TEXT,
    -- 'download' (terminal -> this PC) | 'upload' (this PC -> terminal)
    direction      TEXT NOT NULL,
    -- 'tcp' (direct, port 4370) | 'adms' (the terminal collects a command)
    transport      TEXT NOT NULL DEFAULT 'tcp',
    device_serial  TEXT NOT NULL DEFAULT '',
    device_ip      TEXT NOT NULL DEFAULT '',
    users_seen     INTEGER NOT NULL DEFAULT 0,
    users_written  INTEGER NOT NULL DEFAULT 0,
    fp_seen        INTEGER NOT NULL DEFAULT 0,
    fp_written     INTEGER NOT NULL DEFAULT 0,
    -- How many templates were read back off the device and matched. An upload
    -- that wrote 120 and verified 0 has not worked, however cheerfully the
    -- device acknowledged each packet.
    fp_verified    INTEGER NOT NULL DEFAULT 0,
    ok             INTEGER NOT NULL DEFAULT 0,
    detail         TEXT NOT NULL DEFAULT ''
);
CREATE INDEX idx_bio_sync_started ON bio_sync_runs(started_at);

-- Same reasoning as device_requests: a school that syncs twice a day for ten
-- years should not accumulate an unbounded table.
CREATE TRIGGER trim_bio_sync_runs AFTER INSERT ON bio_sync_runs
BEGIN
    DELETE FROM bio_sync_runs
     WHERE id < (SELECT MAX(id) - 2000 FROM bio_sync_runs);
END;
