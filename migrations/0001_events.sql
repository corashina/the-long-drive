-- Game events from `functions/api/event.js`. `time` is Unix ms. `data` is JSON.
CREATE TABLE events (
  id INTEGER PRIMARY KEY,
  time INTEGER NOT NULL,
  session TEXT NOT NULL,
  email TEXT,
  country TEXT,
  agent TEXT NOT NULL,
  type TEXT NOT NULL,
  build TEXT NOT NULL,
  data TEXT NOT NULL
);
CREATE INDEX events_time ON events (time);
