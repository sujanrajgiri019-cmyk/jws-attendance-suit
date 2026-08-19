-- 004_device_log: a record of everything the terminal says to us.
--
-- Written because a terminal that half-works is the hardest thing to diagnose
-- in this system. Punches were arriving from the K40 Pro while every command
-- queued for it was ignored, and from outside there was no way to tell whether
-- the device was polling for work and finding none, or never polling at all.
-- Those two have completely different fixes and looked identical.
--
-- So every inbound request is logged: which endpoint, what it carried, what we
-- answered. The Diagnose button reads this back, and a question that used to
-- take a day of guessing is answered in one line.

CREATE TABLE device_requests (
    id            INTEGER PRIMARY KEY,
    ts            TEXT NOT NULL DEFAULT (datetime('now','localtime')),
    device_serial TEXT NOT NULL DEFAULT '',
    method        TEXT NOT NULL,
    -- '/iclock/cdata', '/iclock/getrequest', '/iclock/devicecmd'
    endpoint      TEXT NOT NULL,
    -- The `table=` parameter on a POST: ATTLOG, USERINFO, FINGERTMP, ...
    table_name    TEXT NOT NULL DEFAULT '',
    body_bytes    INTEGER NOT NULL DEFAULT 0,
    -- How many records we made of it, so an empty post is distinguishable from
    -- one we failed to parse.
    records       INTEGER NOT NULL DEFAULT 0,
    reply         TEXT NOT NULL DEFAULT ''
);
CREATE INDEX idx_devreq_ts       ON device_requests(ts);
CREATE INDEX idx_devreq_endpoint ON device_requests(endpoint, ts);

-- Keep the table from growing without limit on a terminal that polls every
-- second: a trigger prunes anything beyond the most recent few thousand rows.
CREATE TRIGGER trim_device_requests AFTER INSERT ON device_requests
BEGIN
    DELETE FROM device_requests
     WHERE id < (SELECT MAX(id) - 5000 FROM device_requests);
END;
