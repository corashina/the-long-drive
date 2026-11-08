-- The site has no Access login, so events have no email.
ALTER TABLE events DROP COLUMN email;
