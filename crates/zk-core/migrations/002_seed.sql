-- 002_seed: the defaults a brand-new install needs to be usable immediately.
--
-- Everything here reflects how Janapremi World School actually runs: Sunday to
-- Friday duty, Saturday off, 09:00-16:00 for teaching and administration.

INSERT INTO shifts (id, name, code, start_time, end_time, in_window_start, out_window_end,
                    late_grace, early_grace, break_min, min_full_day, overnight, count_ot)
VALUES
 (1,'Regular Duty',  'REG','09:00','16:00','06:00','21:00',10,10,40,350,0,1),
 (2,'Morning Duty',  'MRN','06:30','12:30','05:00','15:00', 5, 5,20,315,0,1),
 (3,'Support Staff', 'SUP','07:30','17:00','05:30','22:00',15,15,60,480,0,1),
 (4,'Half Day',      'HAF','09:00','12:30','06:00','15:00',10,10, 0,180,0,0),
 (5,'Exam Duty',     'EXM','08:00','17:00','06:00','21:00', 0, 0,45,465,0,1),
 (6,'Night Guard',   'NGT','19:00','06:00','17:00','09:00',15,15, 0,600,1,0);

INSERT INTO timetables (id, name) VALUES
 (1,'Regular Teaching (Sun-Fri)'),
 (2,'Administration (Sun-Fri)'),
 (3,'Support Staff (Sun-Sat)'),
 (4,'Night Security (7 days)');

-- weekday 0=Sun .. 6=Sat. NULL shift = day off.
INSERT INTO timetable_days (timetable_id, weekday, shift_id) VALUES
 (1,0,1),(1,1,1),(1,2,1),(1,3,1),(1,4,1),(1,5,1),(1,6,NULL),
 (2,0,1),(2,1,1),(2,2,1),(2,3,1),(2,4,1),(2,5,1),(2,6,NULL),
 (3,0,3),(3,1,3),(3,2,3),(3,3,3),(3,4,3),(3,5,3),(3,6,4),
 (4,0,6),(4,1,6),(4,2,6),(4,3,6),(4,4,6),(4,5,6),(4,6,6);

INSERT INTO departments (id, name, code, default_timetable_id, colour) VALUES
 (1,'Administration',      'ADM',2,'#F16522'),
 (2,'Primary Level',       'PRI',1,'#2B6CB0'),
 (3,'Lower Secondary',     'LSE',1,'#1F9D55'),
 (4,'Secondary Level',     'SEC',1,'#6B4EBB'),
 (5,'Accounts',            'ACC',2,'#C9820B'),
 (6,'Library & IT',        'LIT',2,'#0E7C86'),
 (7,'Sports & Activities', 'SPT',1,'#B8336A'),
 (8,'Support Staff',       'SUP',3,'#5A6B7B');

-- The terminal already in service at the main gate.
INSERT INTO devices (id, name, machine_no, model, serial, mac, ip, port, mode, location)
VALUES (1,'Main Gate',101,'ZKTeco K40 Pro','GED7253800740','00:17:61:10:c0:77',
        '192.168.100.99',4370,'push','Main Building Entrance');

INSERT INTO rules (key, value) VALUES
 ('working_days','0,1,2,3,4,5'),
 ('late_grace_min','10'),
 ('early_grace_min','10'),
 ('half_day_after_min','120'),
 ('absent_after_min','240'),
 ('min_full_day_min','350'),
 ('first_last_punch','1'),
 ('require_both_punches','1'),
 ('lone_punch_half_day','1'),
 ('dedupe_window_sec','60'),
 ('allow_manual_edit','1'),
 ('lock_after_close','0'),
 ('count_ot','1'),
 ('min_ot_block_min','30'),
 ('ot_needs_approval','0'),
 ('holidays_paid','1'),
 ('sandwich_rule','0'),
 ('late_penalty_enabled','1'),
 ('late_penalty_count','3'),
 ('warn_email_on_3rd_late','1'),
 ('exempt_heads_from_late','0'),
 ('email_absentees','1'),
 ('email_absentees_at','10:00'),
 ('daily_summary_principal','1'),
 ('daily_summary_at','17:00'),
 ('weekly_dept_report','0'),
 ('flag_below_percent','85'),
 ('recompute_on_rule_change','1');

INSERT INTO settings (key, value) VALUES
 ('school_name','Janapremi World School'),
 ('school_address','Madhyapur Thimi-3, Kaushaltar, Bhaktapur'),
 ('school_phone1','9744570500'),
 ('school_phone2','9744570501'),
 ('school_landline','01-5910299'),
 ('school_email','jws.staffattendance@gmail.com'),
 ('admin_username','admin'),
 -- Default password is Attendance@123. Stored as a salted PBKDF2-SHA256 hash;
 -- the app forces a change on first run.
 ('admin_password_hash',''),
 ('admin_password_is_default','1'),
 ('recovery_email','jws.staffattendance@gmail.com'),
 ('smtp_host','smtp.gmail.com'),
 ('smtp_port','587'),
 ('smtp_user','jws.staffattendance@gmail.com'),
 ('smtp_pass',''),
 ('smtp_encryption','starttls'),
 ('calendar_mode','bs_with_ad'),
 ('time_format','24'),
 ('weekly_holiday','6'),
 ('push_port','8081'),
 ('connection_mode','push'),
 ('backup_dir',''),
 ('backup_schedule','daily_18'),
 ('update_repo','sujanrajgiri019-cmyk/jws-attendance-suit'),
 ('update_channel','stable'),
 ('update_check','daily'),
 ('start_with_windows','1'),
 ('minimise_to_tray','1');

-- Nepal public holidays for 2083 BS / 2026-27 AD.
INSERT INTO holidays (name, from_date, to_date) VALUES
 ('Janai Purnima',        '2026-08-28','2026-08-28'),
 ('Gai Jatra',            '2026-08-29','2026-08-29'),
 ('Krishna Janmashtami',  '2026-09-04','2026-09-04'),
 ('Haritalika Teej',      '2026-09-14','2026-09-14'),
 ('Constitution Day',     '2026-09-19','2026-09-19'),
 ('Ghatasthapana',        '2026-10-11','2026-10-11'),
 ('Dashain',              '2026-10-17','2026-10-23'),
 ('Tihar',                '2026-11-07','2026-11-11'),
 ('Chhath',               '2026-11-15','2026-11-15'),
 ('Maghe Sankranti',      '2027-01-15','2027-01-15'),
 ('Maha Shivaratri',      '2027-02-16','2027-02-16'),
 ('Holi',                 '2027-03-04','2027-03-04'),
 ('Nepali New Year 2084', '2027-04-14','2027-04-14');
